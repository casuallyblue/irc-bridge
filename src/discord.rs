use regex::{Captures, Regex};
use serenity::async_trait;
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::prelude::interaction::Interaction;
use serenity::model::prelude::GuildId;
use serenity::model::prelude::Member;
use serenity::model::prelude::UserId;
use serenity::model::prelude::WebhookId;
use serenity::model::user::User;
use serenity::prelude::*;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use crate::IrcRequest;
use crate::Result;

use crate::irc_side::IrcResponseCallback;
use crate::BridgeSenders;
use crate::Config;

pub struct Handler {
    pub config: crate::Config,
    pub irc_sender: irc::client::Sender,
    pub client_ref: Arc<Mutex<irc::client::Client>>,
    pub ignored_users: Vec<UserId>,
    pub webhook_id: WebhookId,
    pub database_pool: SqlitePool,
    pub senders: BridgeSenders,
}

pub async fn discord_receiver(mut discord_client: Client) -> Result<()> {
    discord_client.start().await?;
    Ok(())
}

impl Handler {
    fn should_ignore_message(&self, ctx: &Context, message: &Message) -> bool {
        message.is_own(&ctx.cache)
            || self.ignored_users.contains(&message.author.id)
            || (message.webhook_id == Some(self.webhook_id))
    }

    async fn handle_names_command(&self, ctx: &Context, command: ApplicationCommandInteraction) {
        self.senders
            .irc
            .send(IrcRequest::Names {
                interaction: command,
            })
            .await
            .expect("Could not send message to irc handler");
    }

    async fn handle_connect_user_command(
        &self,
        ctx: &Context,
        member: Option<Member>,
        user: User,
        nick: String,
        command: ApplicationCommandInteraction,
    ) {
        if let Ok(_) = sqlx::query!("SELECT * FROM users WHERE ircnick=?1", nick)
            .fetch_one(&self.database_pool)
            .await
        {
            let user_id_str = user.id.0.to_string();
            sqlx::query!(
                "UPDATE users SET discordid = ?1, discordnick = ?2 WHERE ircnick = ?3",
                user_id_str,
                user.name,
                nick
            )
            .execute(&self.database_pool)
            .await
            .unwrap();
        } else {
            let user_id_str = user.id.0.to_string();
            let name = if let Some(member) = member.clone() {
                member.nick.unwrap_or(user.name.clone())
            } else {
                user.name.clone()
            };
            let discordname = user.name.clone();
            println!("added {}", name);
            sqlx::query!(
                            "INSERT INTO users (ircnick, discordid, discordname, discordnick, verified) VALUES (?1,?2,?3,?4,?5)",
                            nick,
                            discordname,
                            name,
                            user_id_str,
                            false
                        ).execute(&self.database_pool).await.expect("Could not insert record into table");
        }

        command
            .create_interaction_response(&ctx.http, |w| {
                w.interaction_response_data(|w| {
                    w.content(format!("connecting user {nick}")).ephemeral(true)
                })
            })
            .await
            .expect("Could not respond to discord interaction");
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        if !self.should_ignore_message(&ctx, &message) {
            if self.config.discord_channel == message.channel_id.0 {
                let message = make_irc_message(&self.config, message, &ctx).await;

                let request = IrcRequest::SendMessage {
                    to: self.config.irc_channel.clone(),
                    message,
                };

                if let Err(e) = self.senders.irc.send(request).await {
                    println!("Could not send request to irc {e}")
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => match command.data.name.as_str() {
                "connect_user" => {
                    self.handle_connect_user_command(
                        &ctx,
                        command.member.clone(),
                        command.user.clone(),
                        command
                            .data
                            .options
                            .first()
                            .unwrap()
                            .clone()
                            .value
                            .unwrap()
                            .to_string(),
                        command,
                    )
                    .await;
                }
                "users" => self.handle_names_command(&ctx, command).await,

                _ => {}
            },
            _ => {}
        }
    }
}

async fn get_nick_from_user(user: &User, id: GuildId, ctx: &Context) -> String {
    match user.nick_in(ctx.http.clone(), id).await {
        Some(nick) => nick,
        None => user.name.clone(),
    }
}

#[derive(Debug)]
enum FindUsernameCommand {
    Translate(u64),
    End,
}

async fn find_discord_usernames(
    config: Config,
    mut reciever: Receiver<FindUsernameCommand>,
    sender: Sender<String>,
) {
    let http = Http::new_with_application_id(config.discord_token.as_str(), config.application_id);
    while let Some(command) = reciever.recv().await {
        match command {
            FindUsernameCommand::Translate(id) => {
                sender
                    .send(http.get_user(id).await.unwrap().name)
                    .await
                    .expect("Could not send user id to calling thread");
            }
            FindUsernameCommand::End => break,
        }
    }
}

async fn make_irc_message(config: &Config, message: Message, ctx: &Context) -> String {
    let nick = get_nick_from_user(
        &message.author,
        message.guild_id.expect("Message must be sent in a channel"),
        &ctx,
    )
    .await;

    let (task_sender, receiver) = channel(10);
    let (sender, task_receiver) = channel(10);

    let finder = tokio::spawn(find_discord_usernames(
        config.clone(),
        task_receiver,
        task_sender,
    ));

    let message_before_replacement = message.clone();
    let actual_message = async_regex_replace_usernames(message, sender, receiver)
        .await
        .unwrap_or(message_before_replacement.content);

    finder.await.unwrap();

    format!("<{}> {}", nick, actual_message)
}

fn async_regex_replace_usernames(
    message: Message,
    sender: Sender<FindUsernameCommand>,
    mut receiver: Receiver<String>,
) -> tokio::task::JoinHandle<String> {
    tokio::task::spawn_blocking(move || {
        let result = Regex::new(r"<@(\d+)>").expect("Could not compile regex");
        result.replace_all(message.content.as_ref(), |captures: &Captures| {
            let located_user_string = if let Some(user_id) = str::parse::<u64>(&captures[1]).ok() {
                sender
                    .blocking_send(FindUsernameCommand::Translate(user_id))
                    .unwrap();

                receiver.blocking_recv().unwrap_or(user_id.to_string())
            } else {
                captures[1].to_string()
            };

            format!("{}:", located_user_string)
        });

        sender.blocking_send(FindUsernameCommand::End).unwrap();

        return result.to_string();
    })
}
