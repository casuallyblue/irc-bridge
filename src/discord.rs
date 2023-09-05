use regex::{Captures, Regex};
use serenity::async_trait;
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::prelude::interaction::Interaction;
use serenity::model::prelude::GuildId;
use serenity::model::prelude::UserId;
use serenity::model::prelude::WebhookId;
use serenity::model::user::User;
use serenity::prelude::*;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;

use crate::Config;

pub struct Handler {
    pub config: crate::Config,
    pub irc_sender: irc::client::Sender,
    pub client_ref: Arc<Mutex<irc::client::Client>>,
    pub ignored_users: Vec<UserId>,
    pub webhook_id: WebhookId,
    pub database_pool: SqlitePool,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        if !message.is_own(&ctx.cache)
            && !self.ignored_users.contains(&message.author.id)
            && !(message.webhook_id == Some(self.webhook_id))
        {
            if str::parse::<u64>(self.config.discord_channel.as_str()) == Ok(message.channel_id.0) {
                let message = make_irc_message(&self.config, message, &ctx).await;

                self.irc_sender
                    .send_privmsg(self.config.irc_channel.clone(), message)
                    .expect("Cannot send message to irc");
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => match command.data.name.as_str() {
                "connect_user" => {
                    let nick = command.data.options.first().unwrap().clone().value.unwrap();
                    let nick = nick.as_str().unwrap();

                    let nick_c = nick.clone();
                    if let Ok(_) = sqlx::query!("SELECT * FROM users WHERE ircnick=?1", nick_c)
                        .fetch_one(&self.database_pool)
                        .await
                    {
                        let user_id_str = command.user.id.0.to_string();
                        sqlx::query!(
                            "UPDATE users SET discordid = ?1, discordnick = ?2 WHERE ircnick = ?3",
                            user_id_str,
                            command.user.name,
                            nick_c
                        )
                        .execute(&self.database_pool)
                        .await
                        .unwrap();
                    } else {
                        let user_id_str = command.user.id.0.to_string();
                        let name = if let Some(member) = command.member.clone() {
                            member.nick.unwrap_or(command.user.name.clone())
                        } else {
                            command.user.name.clone()
                        };
                        let discordname = command.user.name.clone();
                        println!("added {}", name);
                        sqlx::query!(
                            "INSERT INTO users (ircnick, discordid, discordname, discordnick, verified) VALUES (?1,?2,?3,?4,?5)",
                            nick,
                            discordname,
                            name,
                            user_id_str,
                            false
                        ).execute(&self.database_pool).await.unwrap();
                    }

                    command
                        .create_interaction_response(&ctx.http, |w| {
                            w.interaction_response_data(|w| {
                                w.content(format!("connecting user {nick}")).ephemeral(true)
                            })
                        })
                        .await
                        .unwrap()
                }
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

async fn make_irc_message(config: &Config, message: Message, ctx: &Context) -> String {
    let nick = get_nick_from_user(
        &message.author,
        message.guild_id.expect("Message must be sent in a channel"),
        &ctx,
    )
    .await;

    let re = Regex::new(r"@(\d+)").unwrap();
    /*let result = re.replace_all(message.content.as_str(), |captures: &Captures| {
        let user_id = str::parse::<u64>(&captures[1]).unwrap();

        let http =
            Http::new_with_application_id(config.discord_token.as_str(), config.application_id);

        println!("getting name for user id {}", user_id);
        let name =
            futures::executor::block_on(async { http.get_user(user_id).await.unwrap().name });
        println!("name is {}", name);

        name
    });*/

    format!("<{}> {}", nick, message.content)
}

pub async fn run_discord(mut discordclient: Client) {
    // start listening for events by starting a single shard
    if let Err(why) = discordclient.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}
