-- Add up migration script here
CREATE TABLE IF NOT EXISTS users
(
    ircnick TEXT PRIMARY KEY NOT NULL,
    discordid INTEGER,
    discordnick TEXT,
    discordname TEXT,
    verified BOOLEAN,
    avatar TEXT
);
