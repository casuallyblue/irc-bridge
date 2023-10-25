use clap::{Parser, Subcommand};
use irc::client::ClientStream;
use irc::proto::Response;
use serenity::futures::StreamExt;
use serenity::http::client::*;
use serenity::model::prelude::{GuildId, Member};
use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;

use crate::{BridgeSenders, DiscordRequest};

#[derive(Parser, Clone, Debug)]
enum IrcBotCommand {
    Avatar {
        #[command(subcommand)]
        command: AvatarCommand,
    },
    Connect {
        discord_id: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
enum AvatarCommand {
    Url { url: String },
    Gravatar { email: String },
    Reset,
}

pub fn make_gravatar_url(email: String) -> String {
    let fixed = email.to_lowercase();
    let hash = md5::compute(fixed.as_bytes());
    format!("https://www.gravatar.com/avatar/{hash:x}.jpg?s=128")
}

pub async fn get_avatar_from_guild_member(member: Member) -> String {
    member.avatar_url().unwrap_or(
        member
            .user
            .avatar_url()
            .unwrap_or(member.user.default_avatar_url()),
    )
}

pub async fn select_avatar_for_user(
    pool: &SqlitePool,
    http: &Http,
    guild: GuildId,
    nick: String,
) -> Option<String> {
    if let Some(entry) = lookup_nick_in_database(pool, &nick).await {
        // If a custom avatar is set, use that
        if let Some(avatar) = entry.avatar {
            return Some(avatar);
        // If the User has a discord user associated with their account and is verified, use that
        // user's avatar
        } else if let Some(discord_nick) = entry.discord_nick && entry.verified {
            if let Some(user) = lookup_nick_on_discord(http, guild, discord_nick).await {
                return Some(get_avatar_from_guild_member(user).await);
            }
        // Otherwise check if their username matches one on discord and use that avatar
        } else if let Some(user) = lookup_nick_on_discord(http, guild, nick).await {
            return Some(get_avatar_from_guild_member(user).await);
        }
    }
    // Finally, if nothing matches, just use the default avatar
    None
}

async fn lookup_nick_on_discord(
    http: &Http,
    guild: GuildId,
    discord_nick: String,
) -> Option<Member> {
    guild
        .search_members(http, discord_nick.as_str(), None)
        .await
        .ok()
        .and_then(|members| {
            members.first().and_then(|member| {
                if member.user.name.eq_ignore_ascii_case(discord_nick.as_str()) {
                    Some(member.clone())
                } else {
                    None
                }
            })
        })
}

struct UserInfo {
    verified: bool,
    irc_nick: String,
    discord_id: Option<i64>,
    discord_nick: Option<String>,
    discord_name: Option<String>,
    avatar: Option<String>,
}

async fn lookup_nick_in_database(pool: &SqlitePool, nick: &String) -> Option<UserInfo> {
    let mut conn = pool
        .acquire()
        .await
        .expect("Could not make connection to database");

    sqlx::query!("SELECT * FROM users WHERE ircnick = ?", nick)
        .fetch_one(&mut *conn)
        .await
        .ok()
        .map(|info| UserInfo {
            verified: info.verified.unwrap_or(false),
            irc_nick: info.ircnick,
            discord_id: info.discordid,
            discord_nick: info.discordnick,
            discord_name: info.discordname,
            avatar: info.avatar,
        })
}

#[derive(Debug)]
pub enum IrcResponse {
    NamesResponse(Vec<String>),
}

#[derive(Debug)]
pub struct IrcResponseCallback {
    pub sender: tokio::sync::mpsc::Sender<IrcResponse>,
}

pub async fn irc_receiver(
    mut stream: ClientStream,
    database_pool: SqlitePool,
    config: crate::Config,
    senders: BridgeSenders,
    mut response_callbacks: Receiver<IrcResponseCallback>,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Http::new(&config.discord_token);

    let webhook = http.get_webhook_from_url(&config.discord_webhook).await?;

    let guild = webhook
        .guild_id
        .ok_or("No associated discord guild for webhook")?;

    while let Some(message) = stream.next().await.transpose()? {
        let actual_message = message.clone();

        match message.command {
            irc::proto::Command::Response(Response::RPL_NAMREPLY, data) => {
                println!("Received names reply with content {:?}", data);
                if let Some(callback) = response_callbacks.try_recv().ok() {
                    let _ = callback.sender.send(IrcResponse::NamesResponse(data));
                } else {
                    println!("No callback found");
                }
            }
            irc::proto::Command::PRIVMSG(channel, message) => {
                let Some(nick) = actual_message.source_nickname() else {
                    continue;
                };

                let nick = nick.to_string();
                if config.ignored_irc_users.contains(&nick.to_string()) {
                    continue;
                }

                let username: String;
                let message = message.clone();

                let stored_user = lookup_nick_in_database(&database_pool, &nick).await;

                let user_in_discord = find_member_for_nick(&http, guild, nick.clone()).await;

                if channel == config.irc_channel.clone() {
                    username = if let Some(user) = &stored_user {
                        // If the user is verified to be a discord user use that avatar
                        if user.verified {
                            user.discord_name
                                .clone()
                                .expect("Could not find name on verified user")
                        } else {
                            if user_in_discord.is_some() {
                                let guild_member = user_in_discord.clone().unwrap();
                                guild_member.nick.unwrap_or(guild_member.user.name)
                            } else {
                                nick.clone()
                            }
                        }
                    } else {
                        nick.clone()
                    };

                    // ignore the bridge users
                    if username.contains("discord") {
                        continue;
                    }

                    senders
                        .discord
                        .send(DiscordRequest::SetAvatar {
                            avatar_url: select_avatar_for_user(
                                &database_pool,
                                &http,
                                guild,
                                nick.clone(),
                            )
                            .await,
                        })
                        .await?;

                    senders
                        .discord
                        .send(DiscordRequest::SendMessage {
                            alias: username,
                            message,
                        })
                        .await?;
                } else if channel == config.irc_nick {
                    let mut args: Vec<&str> = message.split_whitespace().collect();
                    let mut args_with_command_name = vec!["bridge"];
                    args_with_command_name.append(&mut args);
                    let command = match IrcBotCommand::try_parse_from(args_with_command_name.iter())
                    {
                        Err(e) => {
                            let pmsg_user = |msg: String| async {
                                senders
                                    .irc
                                    .send(crate::IrcRequest::SendMessage {
                                        to: nick.clone(),
                                        message: msg,
                                    })
                                    .await
                            };
                            println!("{e}");

                            pmsg_user("Error, unknown command".into()).await?;
                            pmsg_user("Valid commands are: ".into()).await?;
                            pmsg_user("> avatar gravatar {email}".into()).await?;
                            pmsg_user("> avatar reset".into()).await?;
                            pmsg_user("> avatar url {url}".into()).await?;
                            continue;
                        }
                        Ok(command) => command,
                    };

                    handle_irc_bot_command(command, stored_user, &database_pool, nick).await?
                }
            }

            _ => {
                println!("Unrecognized message {:?}", message)
            }
        }
    }

    Ok(())
}

async fn handle_irc_bot_command(
    command: IrcBotCommand,
    stored_user: Option<UserInfo>,
    database_pool: &SqlitePool,
    nick: String,
) -> crate::Result<()> {
    match command {
        IrcBotCommand::Avatar { command } => match command {
            AvatarCommand::Url { url } => {
                if let Some(_) = stored_user {
                    sqlx::query!("UPDATE users SET avatar = ?1 WHERE ircnick = ?2", url, nick)
                        .execute(database_pool)
                        .await?;
                } else {
                    sqlx::query!(
                        "INSERT INTO users ( ircnick, avatar) VALUES ( ?1,?2 )",
                        nick,
                        url
                    )
                    .execute(database_pool)
                    .await?;
                }
            }
            AvatarCommand::Gravatar { email } => {
                let avatar_url = make_gravatar_url(email);
                if let Some(_) = stored_user {
                    sqlx::query!(
                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                        avatar_url,
                        nick
                    )
                    .execute(database_pool)
                    .await?;
                } else {
                    sqlx::query!(
                        "INSERT INTO users (ircnick, avatar) VALUES ( ?1,?2 )",
                        nick,
                        avatar_url
                    )
                    .execute(database_pool)
                    .await?;
                }
            }
            AvatarCommand::Reset => {
                if let Some(user) = stored_user && user.avatar.is_some() {
                                    sqlx::query!(
                                        "UPDATE users SET avatar = ?1 WHERE ircnick = ?2",
                                        None::<String>,
                                        nick
                                    ).execute(database_pool)
                                    .await
                                    ?;
                                }
            }
        },
        IrcBotCommand::Connect { discord_id } => {
            if let Some(user) = stored_user {
                if user.discord_nick == Some(discord_id) {
                    sqlx::query!(
                        "UPDATE users SET verified = ?1 WHERE ircnick = ?2",
                        true,
                        nick
                    )
                    .execute(database_pool)
                    .await?;
                }
            }
        }
    }
    Ok(())
}

async fn find_member_for_nick(http: &Http, guild: GuildId, nick: String) -> Option<Member> {
    guild
        .search_members(http, nick.as_str(), None)
        .await
        .ok()
        .and_then(|members| {
            members.first().and_then(|member| {
                if member.user.name.eq_ignore_ascii_case(nick.as_str()) {
                    Some(member.clone())
                } else {
                    None
                }
            })
        })
}
