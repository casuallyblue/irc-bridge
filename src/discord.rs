use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::prelude::interaction::Interaction;
use serenity::model::prelude::GuildId;
use serenity::model::prelude::UserId;
use serenity::model::user::User;
use serenity::prelude::*;
use std::sync::Arc;
use std::sync::Mutex;

pub struct Handler {
    pub config: crate::Config,
    pub irc_sender: irc::client::Sender,
    pub client_ref: Arc<Mutex<irc::client::Client>>,
    pub ignored_users: Vec<UserId>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        if !message.is_own(&ctx.cache) && !self.ignored_users.contains(&message.author.id) {
            match self.config.discord_channels.iter().position(|id| {
                str::parse::<u64>(id).expect("Channel id was not a number") == message.channel_id.0
            }) {
                Some(index) => {
                    let message = make_irc_message(message, &ctx).await;
                    self.irc_sender
                        .send_privmsg(self.config.irc_channels[index].clone(), message)
                        .expect("Cannot send message to irc");
                }
                None => {}
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => match command.data.name.as_str() {
                "connect_user" => {
                    let nick = command.data.options.first().unwrap().clone().value.unwrap();

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
async fn make_irc_message(message: Message, ctx: &Context) -> String {
    let nick = get_nick_from_user(
        &message.author,
        message.guild_id.expect("Message must be sent in a channel"),
        &ctx,
    )
    .await;

    format!("<{}> {}", nick, message.content)
}

pub async fn run_discord(mut discordclient: Client) {
    // start listening for events by starting a single shard
    if let Err(why) = discordclient.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}
