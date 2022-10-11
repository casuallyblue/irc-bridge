use irc::client::ClientStream;
use serenity::futures::StreamExt;
use serenity::http::client::*;

pub async fn run_irc(mut stream: ClientStream, config: crate::Config) {
    let http = Http::new(&config.discord_token);

    while let Some(message) = stream.next().await.transpose().unwrap() {
        let Some(index) = config.irc_channels.iter().position(|chan| chan == message.response_target().unwrap()) else {
            panic!("Expected there to be a discord channel to send to");
        };
        println!("Message get: {}", message);
    }
}
