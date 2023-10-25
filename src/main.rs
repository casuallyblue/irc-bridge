#![feature(let_chains, unboxed_closures, async_closure)]
use clap::Parser;
use irc::{
    client::Sender,
    proto::{Command, Message, Prefix},
};
use irc_side::IrcResponseCallback;
use serenity::{framework::StandardFramework, http::Http, model::webhook::Webhook, prelude::*};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio::{
    select,
    sync::mpsc::{channel, Receiver},
};

mod discord;
mod irc_side;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[clap(env = "BRIDGE_IRC_NICK")]
    irc_nick: String,

    #[clap(env = "BRIDGE_IRC_HOST")]
    irc_host: String,

    #[clap(env = "BRIDGE_IRC_PORT")]
    irc_port: String,

    #[clap(env = "BRIDGE_IRC_CHANNEL")]
    irc_channel: String,

    #[clap(env = "BRIDGE_DISCORD_TOKEN")]
    discord_token: String,

    #[clap(env = "BRIDGE_DISCORD_APPID")]
    application_id: u64,

    #[clap(env = "BRIDGE_DISCORD_WEBHOOK")]
    discord_webhook: String,

    #[clap(env = "BRIDGE_DISCORD_CHANNEL")]
    discord_channel: u64,

    #[clap(env = "BRIDGE_SQLITE_PATH")]
    sqlite_path: String,

    #[clap(env = "IRC_IGNORED_USERS", long = "irc_ignored")]
    ignored_irc_users: Vec<String>,

    #[clap(env = "DISCORD_IGNORED_USERS", long = "discord_ignored")]
    ignored_discord_users: Vec<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::parse();

    println!("LOG: READ CONFIG");

    let irc_config = irc::client::prelude::Config {
        nickname: Some(config.irc_nick.clone()),
        server: Some(config.irc_host.clone()),
        port: Some(
            str::parse(&config.irc_port.clone()).expect("Must be an integer between 0 and 65536"),
        ),
        channels: vec![config.irc_channel.clone()],
        use_tls: Some(true),
        ..Default::default()
    };

    println!("LOG: Connecting to irc");

    let mut client = irc::client::Client::from_config(irc_config)
        .await
        .expect("Cannot connect to irc");
    println!("LOG: Identifying to irc server");
    client.identify()?;

    let pool = SqlitePool::connect(&config.sqlite_path).await?;

    println!("LOG: Connected to irc");

    let sender = client.sender();
    let stream = client.stream().expect("Cannot get stream");

    let clientref = Arc::new(Mutex::new(client));

    let http = Http::new_with_application_id(&config.discord_token, config.application_id);

    let webhook = Webhook::from_url(&http, &config.discord_webhook).await?;

    let (irc_command_sender, irc_command_receiver) = channel(20);
    let (discord_command_sender, discord_command_receiver) = channel(20);
    let (irc_response_callback_sender, irc_response_callback_receiver) = channel(20);
    let senders = BridgeSenders {
        irc: irc_command_sender.clone(),
        discord: discord_command_sender.clone(),
        irc_callback: irc_response_callback_sender.clone(),
    };

    let handler = discord::Handler {
        config: config.clone(),
        irc_sender: sender.clone(),
        client_ref: clientref.clone(),
        ignored_users: vec![1021460721239867535.into()],
        webhook_id: webhook.id,
        database_pool: pool.clone(),
        senders: senders.clone(),
    };

    println!("LOG: Created discord handler");

    let framework = StandardFramework::new().configure(|c| c.prefix("~"));

    // Login with a bot token from the environment
    let token = handler.config.discord_token.clone();
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let discord_client = Client::builder(token, intents)
        .event_handler(handler)
        .framework(framework)
        .await
        .expect("Error creating client");

    register_discord_slash_commands(config.clone()).await?;

    let _ = select! {
        Ok(()) = discord_sender(config.clone(), discord_command_receiver) => {},
        Ok(()) = discord::discord_receiver(discord_client) => {},
        Ok(()) = irc_side::irc_receiver(stream, pool.clone(), config.clone(), senders.clone(), irc_response_callback_receiver) => {}
        Ok(()) = irc_sender(config.clone(), sender.clone(), irc_command_receiver, senders.clone()) => {},
    };

    Ok(())
}

async fn register_discord_slash_commands(config: Config) -> Result<()> {
    let http = Http::new_with_application_id(&config.discord_token, config.application_id);

    let guild = http.get_guild(541017705356984330).await?;

    guild
        .create_application_command(&http, |command| {
            command
                .name("connect_user")
                .description("Connect your discord username to a irc nick")
                .create_option(|option| {
                    option
                        .name("nick")
                        .description("the nick to use")
                        .kind(serenity::model::prelude::command::CommandOptionType::String)
                        .required(true)
                })
        })
        .await?;

    guild
        .create_application_command(&http, |command| {
            command
                .name("users")
                .description("Show the currently logged in users in the irc channel")
        })
        .await?;

    Ok(())
}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug)]
pub struct BridgeSenders {
    irc: tokio::sync::mpsc::Sender<IrcRequest>,
    discord: tokio::sync::mpsc::Sender<DiscordRequest>,
    irc_callback: tokio::sync::mpsc::Sender<IrcResponseCallback>,
}

#[derive(Debug)]
pub enum IrcRequest {
    SendMessage { to: String, message: String },
    Names { callback: IrcResponseCallback },
}

#[derive(Debug)]
pub enum DiscordRequest {
    SendMessage { alias: String, message: String },
    SetAvatar { avatar_url: Option<String> },
}

async fn discord_sender(config: Config, mut commands: Receiver<DiscordRequest>) -> Result<()> {
    let http = Http::new(&config.discord_token);

    let mut webhook = http.get_webhook_from_url(&config.discord_webhook).await?;

    while let Some(command) = commands.recv().await {
        match command {
            DiscordRequest::SendMessage { alias, message } => {
                webhook
                    .execute(&http, false, |webhook| {
                        webhook.content(message).username(alias)
                    })
                    .await?;
            }
            DiscordRequest::SetAvatar { avatar_url } => match avatar_url {
                Some(avatar) => webhook.edit_avatar(&http, avatar.as_str()).await?,
                None => webhook.delete_avatar(&http).await?,
            },
        }
    }
    Ok(())
}

async fn irc_sender(
    config: Config,
    sender: Sender,
    mut commands: Receiver<IrcRequest>,
    senders: BridgeSenders,
) -> Result<()> {
    while let Some(command) = commands.recv().await {
        match command {
            IrcRequest::SendMessage { to, message } => sender.send_privmsg(to, message)?,
            IrcRequest::Names { callback } => {
                println!("Got request to get names from irc");
                senders.irc_callback.send(callback).await?;
                println!("Setup callback");
                let message = Message {
                    tags: None,
                    prefix: None,
                    command: Command::NAMES(Some(config.irc_channel.clone()), None),
                };
                println!("sending '{}' to irc", message);
                sender.send(message)?;
                println!("sent names command to irc");
            }
        }
    }
    Ok(())
}
