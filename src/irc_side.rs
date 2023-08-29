use irc::client::ClientStream;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::{
    client::CacheAndHttp,
    model::prelude::application_command::ApplicationCommandInteractionDataOption,
};
use std::sync::{Arc, Mutex};

pub async fn run_irc(
    mut stream: ClientStream,
    irc: Arc<Mutex<irc::client::Client>>,
    cache: Arc<CacheAndHttp>,
    config: crate::Config,
) {
    let http = Http::new(&config.discord_token);

    let mut webhook = http
        .get_webhook_from_url(&config.discord_webhook)
        .await
        .unwrap();

    let guild = webhook.guild_id.unwrap();

    while let Some(message) = stream.next().await.transpose().unwrap() {
        let Some(nick) = message.source_nickname() else {
            continue;
        };

        if config.ignored_irc_users.contains(&nick.to_string()) {
            continue;
        }

        if let irc::client::prelude::Command::PRIVMSG(channel, message) = message.command.clone() {
            if channel == "#openutd" {
                if let Ok(members) = guild.search_members(&http, nick, None).await && members.len() > 0 {
                    let c = members.first().unwrap();

                    if c.user.name.eq_ignore_ascii_case(nick) {

                        let avi = c.user.avatar_url().unwrap();

                        webhook.edit_avatar(&http, avi.as_str()).await.unwrap();
                    } else {
                        webhook.delete_avatar(&http).await.unwrap();
                    }

                } else {
                    webhook.delete_avatar(&http).await.unwrap();
                }

                webhook
                    .execute(&http, false, |w| w.content(message))
                    .await
                    .unwrap();
            }
        }
    }
}
