use serenity::async_trait;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{StandardFramework, CommandResult};

pub struct Handler {
    pub config: crate::Config,
    pub irc_sender: irc::client::Sender
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        if str::parse::<u64>(&self.config.discord_channels[0]).unwrap() == message.channel_id.0 {
            print!("discord message get: {}", message.content);
            self.irc_sender.send_privmsg(self.config.irc_channels[0].clone(), message.content).expect("Cannot send message");
        }
    }
}

pub async fn run_discord(handler: Handler) {
    let framework = StandardFramework::new()
        .configure(|c| c.prefix("~")); // set the bot's prefix to "~"

    // Login with a bot token from the environment
    let token = handler.config.discord_token.clone();
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(token, intents)
        .event_handler(handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    // start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        println!("An error occurred while running the client: {:?}", why);
    }
}
