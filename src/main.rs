use clap::Parser;

mod discord;
mod irc_side;

#[derive(Parser,Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[clap(env="BRIDGE_IRC_NICK")]
    irc_nick: String,

    #[clap(env="BRIDGE_IRC_HOST")]
    irc_host: String,

    #[clap(env="BRIDGE_IRC_PORT")]
    irc_port: String,

    #[clap(env="BRIDGE_IRC_CHANNELS")]
    irc_channels: Vec<String>,

    #[clap(env="BRIDGE_DISCORD_TOKEN")]
    discord_token: String,

    #[clap(env="BRIDGE_DISCORD_WEBHOOK")]
    discord_webhook: String,

    #[clap(env="BRIDGE_DISCORD_CHANNELS")]
    discord_channels: Vec<String>,
}

#[tokio::main]
async fn main() {
    let config = Config::parse();

    println!("LOG: config: {:?}", config);

    println!("LOG: READ CONFIG");

    let irc_config = irc::client::prelude::Config {
        nickname: Some(config.irc_nick.clone()),
        server: Some(config.irc_host.clone()),
        port: Some(str::parse(&config.irc_port.clone()).expect("Must be an integer between 0 and 65536")),
        channels: config.irc_channels.clone(),
        use_tls: Some(true),
        ..Default::default()
    };
    
    let mut client = irc::client::Client::from_config(irc_config).await.expect("Cannot connect to irc");
    client.identify().unwrap();

    println!("LOG: Connected to irc");

    let sender = client.sender();
    let stream = client.stream().expect("Cannot get stream");

    let handler = discord::Handler {
        config: config.clone(),
        irc_sender: sender, 
    };

    println!("LOG: Created discord handler");

    tokio::select!(
        _ = discord::run_discord(handler) => {}
        _ = irc_side::run_irc(stream, config.clone()) => {}
    );
}
