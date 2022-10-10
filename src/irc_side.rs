use irc::client::ClientStream;
use serenity::futures::StreamExt;

pub async fn run_irc(mut stream: ClientStream, config: crate::Config) {
    while let Some(message) = stream.next().await.transpose().unwrap() {
        println!("Message get: {}", message);
    }
}
