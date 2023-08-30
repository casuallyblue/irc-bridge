use irc::client::ClientStream;
use serenity::client::CacheAndHttp;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::model::prelude::GuildId;
use serenity::model::user::User;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub async fn run_irc(
    mut stream: ClientStream,
    irc: Arc<Mutex<irc::client::Client>>,
    cache: Arc<CacheAndHttp>,
    database_pool: SqlitePool,
    config: crate::Config,
) {
    let http = Http::new(&config.discord_token);

    let mut webhook = http
        .get_webhook_from_url(&config.discord_webhook)
        .await
        .unwrap();

    let guild = webhook.guild_id.unwrap();

    while let Some(message) = stream.next().await.transpose().unwrap() {
        let message = message.clone();
        let Some(nick) = message.source_nickname() else {
            continue;
        };

        let nick = nick.to_string();

        if config.ignored_irc_users.contains(&nick.to_string()) {
            continue;
        }

        if let irc::client::prelude::Command::PRIVMSG(channel, message) = message.command.clone() {
            let username: String;
            let message = message.clone();

            let guild_member = find_member_for_nick(&http, guild, nick.clone()).await;

            let mut conn = database_pool.acquire().await.unwrap();

            let nick_c = nick.clone();
            let user_in_db = sqlx::query!("SELECT * FROM users WHERE ircnick = ?", nick_c)
                .fetch_one(&mut *conn)
                .await;

            if channel == config.irc_channel {
                if let Ok(user) = &user_in_db && user.verified == Some(true) {
                    username = user.discordnick.clone().unwrap();
                } else {
                    if let Some(guild_member) = guild_member.clone() {
                        username = guild_member.name;
                    } else {
                        username = nick.clone().into();
                    }
                }

                if username.contains("discord") {
                    continue;
                }

                if let Ok(user) = user_in_db && user.avatar.is_some() {
                    webhook.edit_avatar(&http, user.avatar.unwrap().as_str()).await.unwrap();
                } else if let Some(guild_member) = guild_member {
                    webhook
                        .edit_avatar(&http, guild_member.avatar_url().unwrap().as_str())
                        .await
                        .unwrap();
                } else {
                    webhook.delete_avatar(&http).await.unwrap();
                }

                webhook
                    .execute(&http, false, |w| w.content(message).username(username))
                    .await
                    .unwrap();
            } else if channel == config.irc_nick {
                let parts: Vec<&str> = message.split_whitespace().collect();
                let command_parts = parts.len();

                if command_parts >= 2 {
                    if parts[0] == "avatar" {
                        if command_parts == 3 && parts[1] == "gravatar" {
                            let fixed = parts[2].trim().to_lowercase();
                            let hash = md5::compute(fixed.as_bytes());
                            let nick_c = nick.clone();
                            let avatar_url =
                                format!("https://www.gravatar.com/avatar/{hash:x}.jpg?s=128");
                            if let Ok(_) = user_in_db {
                                sqlx::query!(
                                    "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                    avatar_url,
                                    nick_c
                                )
                                .execute(&database_pool)
                                .await
                                .unwrap();
                            } else {
                                sqlx::query!(
                                    "INSERT INTO users ( ircnick, avatar) VALUES ( ?1,?2 )",
                                    nick_c,
                                    avatar_url
                                )
                                .execute(&database_pool)
                                .await
                                .unwrap();
                            }
                            continue;
                        } else if command_parts == 2 {
                            if parts[1] == "reset" {
                                if let Ok(user) = user_in_db && user.avatar.is_some() {
                                    sqlx::query!(
                                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                        None::<String>,
                                        nick_c
                                    ).execute(&database_pool)
                                    .await
                                    .unwrap();
                                }
                                continue;
                            } else {
                                if let Ok(_) = user_in_db {
                                    sqlx::query!(
                                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                        parts[1],
                                        nick_c
                                    )
                                    .execute(&database_pool)
                                    .await
                                    .unwrap();
                                } else {
                                    sqlx::query!(
                                        "INSERT INTO users ( ircnick, avatar) VALUES ( ?1,?2 )",
                                        nick_c,
                                        parts[1]
                                    )
                                    .execute(&database_pool)
                                    .await
                                    .unwrap();
                                }
                                continue;
                            }
                        }
                    }
                }

                let pmsg_user = |msg: &str| {
                    if let Ok(irc) = irc.lock() {
                        irc.send(irc::client::prelude::Command::PRIVMSG(
                            nick.clone(),
                            msg.into(),
                        ))
                        .unwrap();
                    }
                };

                pmsg_user("Error, unknown command");
                pmsg_user("Valid commands are: ");
                pmsg_user("> avatar gravatar {email}");
                pmsg_user("> avatar reset");
                pmsg_user("> avatar {url}");
            }
        }
    }
}

#[derive(clap::Parser)]
#[command(author, version, about, long_about=None)]
pub struct IrcBotCommand {
    #[command(subcommand)]
    command: IrcBotSubCommands,
}

#[derive(clap::Subcommand, Clone)]
pub enum IrcBotSubCommands {
    #[command(flatten)]
    Avatar(AvatarCommand),
}

#[derive(clap::Subcommand, Clone)]
pub enum AvatarCommand {
    Reset,
    Gravatar { email: String },
    Url { url: String },
}

async fn find_member_for_nick(http: &Http, guild: GuildId, nick: String) -> Option<User> {
    if let Ok(members) = guild.search_members(http, nick.as_str(), None).await && members.len() > 0 {
        let c = members.first().unwrap();
        if c.user.name.eq_ignore_ascii_case(nick.as_str()) {
            Some(c.user.clone())
        } else {
            None
        }
    } else {
        None
    }
}
