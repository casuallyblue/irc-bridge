#![feature(let_chains)]
use clap::Parser;
use serenity::{
    framework::StandardFramework, http::Http,
    model::prelude::application_command::ApplicationCommand, prelude::*,
};
use std::sync::{Arc, Mutex};

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

    #[clap(env = "BRIDGE_IRC_CHANNELS")]
    irc_channels: Vec<String>,

    #[clap(env = "BRIDGE_DISCORD_TOKEN")]
    discord_token: String,

    #[clap(env = "BRIDGE_DISCORD_APPID")]
    application_id: u64,

    #[clap(env = "BRIDGE_DISCORD_WEBHOOK")]
    discord_webhook: String,

    #[clap(env = "BRIDGE_DISCORD_CHANNELS")]
    discord_channels: Vec<String>,

    #[clap(env = "IRC_IGNORED_USERS")]
    ignored_irc_users: Vec<String>,

    #[clap(env = "DISCORD_IGNORED_USERS")]
    ignored_discord_users: Vec<u64>,
}

#[tokio::main]
async fn main() {
    let config = Config::parse();

    println!("LOG: config: {:?}", config);

    println!("LOG: READ CONFIG");

    let irc_config = irc::client::prelude::Config {
        nickname: Some(config.irc_nick.clone()),
        server: Some(config.irc_host.clone()),
        port: Some(
            str::parse(&config.irc_port.clone()).expect("Must be an integer between 0 and 65536"),
        ),
        channels: config.irc_channels.clone(),
        use_tls: Some(true),
        ..Default::default()
    };

    let mut client = irc::client::Client::from_config(irc_config)
        .await
        .expect("Cannot connect to irc");
    client.identify().unwrap();

    println!("LOG: Connected to irc");

    let sender = client.sender();
    let stream = client.stream().expect("Cannot get stream");

    let clientref = Arc::new(Mutex::new(client));

    let handler = discord::Handler {
        config: config.clone(),
        irc_sender: sender,
        client_ref: clientref.clone(),
        ignored_users: vec![1021460721239867535.into()],
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

    let httpcache = discord_client.cache_and_http.clone();

    register_discord_slash_commands(config.clone()).await;

    tokio::select!(
        _ = discord::run_discord(discord_client) => {}
        _ = irc_side::run_irc(stream, clientref.clone(), httpcache, config.clone()) => {}
    );
}

async fn register_discord_slash_commands(config: Config) {
    let http = Http::new_with_application_id(&config.discord_token, config.application_id);

    let guild = http.get_guild(541017705356984330).await.unwrap();

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
        .await
        .unwrap();
}
