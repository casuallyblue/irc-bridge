use irc::client::ClientStream;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::model::prelude::{GuildId, Member};
use sqlx::SqlitePool;

use crate::{BridgeSenders, DiscordRequest};

pub async fn irc_receiver(
    mut stream: ClientStream,
    database_pool: SqlitePool,
    config: crate::Config,
    senders: BridgeSenders
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Http::new(&config.discord_token);

    let webhook = http
        .get_webhook_from_url(&config.discord_webhook)
        .await?;

    let guild = webhook.guild_id.ok_or("No associated discord guild for webhook")?;


    while let Some(message) = stream.next().await.transpose()? {
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

            let mut conn = database_pool.acquire().await?;

            let nick_c = nick.clone();
            let stored_user = sqlx::query!("SELECT * FROM users WHERE ircnick = ?", nick_c)
                .fetch_one(&mut *conn)
                .await;

            if channel == config.irc_channel.clone() {
                username = if let Ok(user) = &stored_user {
                    if user.verified.unwrap_or(false) {
                        user.discordname.clone().expect("Could not find name on verified user")
                    } else {
                        if guild_member.is_some() {
                            let guild_member = guild_member.clone().unwrap();
                            guild_member.nick.unwrap_or(guild_member.user.name)
                        }  else {
                            nick
                        }
                    }
                } else {
                    nick
                };
                
                 
                if username.contains("discord") {
                    continue;
                }

                if let Ok(user) = stored_user && user.avatar.is_some() {
                    senders.discord.send(DiscordRequest::SetAvatar { avatar_url: Some(user.avatar.unwrap()) }).await?;
                } else if let Some(guild_member) = guild_member {
                    let avatar = if let Some(avatar) = guild_member.avatar_url() {
                        avatar
                    } else {
                        guild_member.user.avatar_url().unwrap_or(guild_member.user.default_avatar_url())        
                    };

                    senders.discord.send(DiscordRequest::SetAvatar { avatar_url: Some(avatar) }).await?;
                } else {
                    senders.discord.send(DiscordRequest::SetAvatar { avatar_url: None }).await?;
                }

                senders.discord.send(DiscordRequest::SendMessage { alias: username, message}).await?;
                } else if channel == config.irc_nick {
                let parts: Vec<&str> = message.split_whitespace().collect();
                let command_parts = parts.len();

                if command_parts >= 2 {
                    if parts[0] == "connect" {
                        if command_parts == 2 {
                            if let Ok(user) = stored_user {
                                if user.discordnick == Some(parts[1].to_string()) {
                                    let nick_c = nick.clone();
                                    sqlx::query!(
                                        "UPDATE users SET verified = ?1 WHERE ircnick = ?2",
                                        true,
                                        nick_c
                                    )
                                    .execute(&database_pool)
                                    .await
                                    ?;
                                    continue;
                                }
                                continue;
                            }
                        }
                    } else if parts[0] == "avatar" {
                        if command_parts == 3 && parts[1] == "gravatar" {
                            let fixed = parts[2].trim().to_lowercase();
                            let hash = md5::compute(fixed.as_bytes());
                            let nick_c = nick.clone();
                            let avatar_url =
                                format!("https://www.gravatar.com/avatar/{hash:x}.jpg?s=128");
                            if let Ok(_) = stored_user {
                                sqlx::query!(
                                    "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                    avatar_url,
                                    nick_c
                                )
                                .execute(&database_pool)
                                .await
                                ?;
                            } else {
                                sqlx::query!(
                                    "INSERT INTO users ( ircnick, avatar) VALUES ( ?1,?2 )",
                                    nick_c,
                                    avatar_url
                                )
                                .execute(&database_pool)
                                .await
                                ?;
                            }
                            continue;
                        } else if command_parts == 2 {
                            if parts[1] == "reset" {
                                if let Ok(user) = stored_user && user.avatar.is_some() {
                                    sqlx::query!(
                                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                        None::<String>,
                                        nick_c
                                    ).execute(&database_pool)
                                    .await
                                    ?;
                                }
                                continue;
                            } else {
                                if let Ok(_) = stored_user {
                                    sqlx::query!(
                                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                        parts[1],
                                        nick_c
                                    )
                                    .execute(&database_pool)
                                    .await
                                    ?;
                                } else {
                                    sqlx::query!(
                                        "INSERT INTO users ( ircnick, avatar) VALUES ( ?1,?2 )",
                                        nick_c,
                                        parts[1]
                                    )
                                    .execute(&database_pool)
                                    .await
                                    ?;
                                }
                                continue;
                            }
                        }
                    }
                }

                let pmsg_user = |msg: String| async {
                    senders.irc.send(crate::IrcRequest::SendMessage { to: nick.clone(), message: msg }).await
                };

                pmsg_user("Error, unknown command".into()).await?;
                pmsg_user("Valid commands are: ".into()).await?;
                pmsg_user("> avatar gravatar {email}".into()).await?;
                pmsg_user("> avatar reset".into()).await?;
                pmsg_user("> avatar {url}".into()).await?;
            }
        }
    }

    Ok(())
}

async fn find_member_for_nick(http: &Http, guild: GuildId, nick: String) -> Option<Member> {
    guild.search_members(http, nick.as_str(), None).await.ok().and_then(|members| {
        members.first().and_then(|member| {
            if member.user.name.eq_ignore_ascii_case(nick.as_str()) {
                Some(member.clone())
            } else {
                None
            }
        })
    })
}
