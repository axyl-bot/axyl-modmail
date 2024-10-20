use std::env;

pub struct Config {
    pub token: String,
    pub forum_channel_id: u64,
    pub role_id: u64,
}

impl Config {
    pub fn get() -> Self {
        Self {
            token: env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN must be set"),
            forum_channel_id: env::var("FORUM_CHANNEL_ID")
                .expect("FORUM_CHANNEL_ID must be set")
                .parse()
                .expect("FORUM_CHANNEL_ID must be a valid u64"),
            role_id: env::var("ROLE_ID")
                .expect("ROLE_ID must be set")
                .parse()
                .expect("ROLE_ID must be a valid u64"),
        }
    }
}
