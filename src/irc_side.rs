use irc::client::ClientStream;
use irc::proto::Command;
use linkify::LinkFinder;
use serenity::client::CacheAndHttp;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::model::prelude::{GuildId, Member};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};

pub async fn run_irc(
    mut stream: ClientStream,
    irc: Arc<Mutex<irc::client::Client>>,
    _cache: Arc<CacheAndHttp>,
    database_pool: SqlitePool,
    config: crate::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Http::new(&config.discord_token);

    let mut webhook = http
        .get_webhook_from_url(&config.discord_webhook)
        .await?;

    let guild = webhook.guild_id.ok_or("No associated discord guild for webhook")?;

    let linkfinder = LinkFinder::new();

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
            let user_in_db = sqlx::query!("SELECT * FROM users WHERE ircnick = ?", nick_c)
                .fetch_one(&mut *conn)
                .await;

            if channel == config.irc_channel.clone() {
                if let Ok(user) = &user_in_db && user.verified == Some(true) {
                    username = user.discordname.clone().ok_or("Expected a username to exist on record")?;
                } else {
                    if let Some(guild_member) = guild_member.clone() {
                        username = guild_member.nick.unwrap_or(guild_member.user.name);
                    } else {
                        username = nick.clone().into();
                    }
                }

                if username.contains("discord") {
                    continue;
                }

                if let Ok(user) = user_in_db && user.avatar.is_some() {
                    webhook.edit_avatar(&http, user.avatar.ok_or("Expected an avatar since is_some returned true")?.as_str()).await?;
                } else if let Some(guild_member) = guild_member {
                    let avatar = if let Some(avatar) = guild_member.avatar_url() {
                        avatar
                    } else {
                        guild_member.user.avatar_url().unwrap_or(guild_member.user.default_avatar_url())        
                    };

                    webhook
                        .edit_avatar(&http, avatar.as_str())
                        .await
                        ?;
                } else {
                    webhook.delete_avatar(&http).await?;
                }

                webhook
                    .execute(&http, false, |w| w.content(message.clone()).username(username))
                    .await
                    ?;
                
                for link in linkfinder.links(message.as_str()) {
                    let og = opengraph::scrape(link.as_str(), opengraph::Opts::default());

                    if let Ok(object) = og {

                    
                    if let Ok(irc) = irc.lock() {
                        irc.send(Command::PRIVMSG(config.irc_channel.clone(), format!("[{}]",object.title)))?;
                    }
                    }
                }
            } else if channel == config.irc_nick {
                let parts: Vec<&str> = message.split_whitespace().collect();
                let command_parts = parts.len();

                if command_parts >= 2 {
                    if parts[0] == "connect" {
                        if command_parts == 2 {
                            if let Ok(user) = user_in_db {
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
                            if let Ok(_) = user_in_db {
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
                                if let Ok(user) = user_in_db && user.avatar.is_some() {
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
                                if let Ok(_) = user_in_db {
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

                let pmsg_user = |msg: &str| {
                    if let Ok(irc) = irc.lock() {
                        irc.send(irc::client::prelude::Command::PRIVMSG(
                            nick.clone(),
                            msg.into(),
                        ))?
                        ;
                    }

                    let result: Result<(), Box<dyn std::error::Error>> = Ok(());
                    
                    result
                };

                pmsg_user("Error, unknown command")?;
                pmsg_user("Valid commands are: ")?;
                pmsg_user("> avatar gravatar {email}")?;
                pmsg_user("> avatar reset")?;
                pmsg_user("> avatar {url}")?;
            }
        }
    }

    Ok(())
}

async fn find_member_for_nick(http: &Http, guild: GuildId, nick: String) -> Option<Member> {
    if let Ok(members) = guild.search_members(http, nick.as_str(), None).await && members.len() > 0 {
        let c = members.first()?;
        if c.user.name.eq_ignore_ascii_case(nick.as_str()) {
            Some(c.clone())
        } else {
            None
        }
    } else {
        None
    }
}
