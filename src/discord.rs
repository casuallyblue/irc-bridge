use serenity::async_trait;
use serenity::model::user::User;
use serenity::model::prelude::GuildId;
use std::sync::Arc;
use serenity::prelude::*;
use std::sync::Mutex;
use serenity::model::channel::Message;

pub struct Handler {
    pub config: crate::Config,
    pub irc_sender: irc::client::Sender,
    pub client_ref: Arc<Mutex<irc::client::Client>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        match self.config.discord_channels
            .iter()
            .position(|id| {str::parse::<u64>(id).expect("Channel id was not a number") == message.channel_id.0}) {
            Some(index) => {
                let message = make_irc_message(message, &ctx).await;
                self.irc_sender
                    .send_privmsg(self.config.irc_channels[index].clone(), message)
                    .expect("Cannot send message to irc");
            }
            None => {
            }
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
        &ctx).await;

    format!("<{}> {}", nick, message.content)
}

pub async fn run_discord(mut discordclient: Client) {
    
    // start listening for events by starting a single shard
    if let Err(why) = discordclient.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}
