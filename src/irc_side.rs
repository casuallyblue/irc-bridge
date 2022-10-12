use irc::client::ClientStream;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::client::CacheAndHttp;
use std::sync::{Arc, Mutex};

pub async fn run_irc(
    mut stream: ClientStream,
    irc: Arc<Mutex<irc::client::Client>>,
    cache: Arc<CacheAndHttp>,
    config: crate::Config,
) {
    let http = Http::new(&config.discord_token);

    let webhook = http.get_webhook_from_url(&config.discord_webhook);

    while let Some(message) = stream.next().await.transpose().unwrap() {
        let Some(index) = config.irc_channels.iter().position(|chan| chan == message.response_target().unwrap()) else {
            panic!("Expected there to be a discord channel to send to");
        };

        let Some(nick) = message.source_nickname() else {
            continue;
        };

        println!("Message get: {}", message);
    }
}
