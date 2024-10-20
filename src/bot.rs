use crate::{commands::*, config::Config};
use serenity::{
    all::*,
    async_trait,
    builder::CreateInteractionResponse,
    model::{gateway::Ready, user::OnlineStatus},
};
use std::sync::Arc;
use tokio::sync::Mutex;

struct Handler {
    state: Arc<Mutex<ModmailState>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            let content = match command.data.name.as_str() {
                "modmail" => modmail(&ctx, &command, self.state.clone()).await,
                "close" => close_thread(&ctx, &command, self.state.clone()).await,
                _ => "Not implemented".to_string(),
            };

            if let Err(why) = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::default().content(content),
                    ),
                )
                .await
            {
                println!("Cannot respond to slash command: {}", why);
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.guild_id.is_none() {
            handle_dm(&ctx, &msg, self.state.clone()).await;
        } else if let Some(thread) = msg
            .channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|c| c.guild())
        {
            if thread.kind == ChannelType::PublicThread {
                handle_thread_message(&ctx, &msg, self.state.clone()).await;
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        match resync_state(&ctx, self.state.clone()).await {
            Ok(_) => println!("State resynced successfully"),
            Err(e) => eprintln!("Error resyncing state: {}", e),
        }

        ctx.set_presence(
            Some(ActivityData::custom("DM me to contact staff")),
            OnlineStatus::DoNotDisturb,
        );

        if let Err(why) = Command::set_global_commands(&ctx.http, vec![]).await {
            println!("Failed to delete global commands: {:?}", why);
            return;
        }

        println!("Deleted all existing global commands.");

        let commands = vec![
            CreateCommand::new("modmail")
                .description("Send a modmail")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "message",
                        "The message to send as modmail",
                    )
                    .required(true),
                ),
            CreateCommand::new("close").description("Close the current modmail thread"),
        ];

        match Command::set_global_commands(&ctx.http, commands).await {
            Ok(cmds) => println!("Successfully registered {} global commands", cmds.len()),
            Err(why) => println!("Failed to register global commands: {:?}", why),
        }
    }
}

pub async fn run_bot() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::get();
    let token = &config.token;
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::DIRECT_MESSAGES;

    let state = Arc::new(Mutex::new(ModmailState {
        user_to_thread: std::collections::HashMap::new(),
        thread_to_user: std::collections::HashMap::new(),
    }));

    let mut client = Client::builder(token, intents)
        .event_handler(Handler {
            state: state.clone(),
        })
        .await?;

    client.start().await?;

    Ok(())
}

pub async fn resync_state(ctx: &Context, state: Arc<Mutex<ModmailState>>) -> Result<(), String> {
    let config = Config::get();
    let forum_channel_id = config.forum_channel_id;

    let forum_channel = ChannelId::new(forum_channel_id)
        .to_channel(&ctx.http)
        .await
        .map_err(|_| "Could not find the specified forum channel".to_string())?;

    if let Channel::Guild(channel) = forum_channel {
        if channel.kind != ChannelType::Forum {
            return Err("The specified channel is not a forum channel".to_string());
        }

        let threads = ctx
            .http
            .get_guild_active_threads(channel.guild_id)
            .await
            .map_err(|e| format!("Failed to fetch threads: {}", e))?;

        let mut new_state = ModmailState {
            user_to_thread: std::collections::HashMap::new(),
            thread_to_user: std::collections::HashMap::new(),
        };

        for thread in threads.threads {
            if thread.parent_id == Some(channel.id) {
                println!("Processing thread: {}", thread.id);
                if let Ok(mut messages) = thread
                    .id
                    .messages(&ctx.http, GetMessages::default().limit(100))
                    .await
                {
                    messages.sort_by_key(|m| m.timestamp);
                    if let Some(first_message) = messages.first() {
                        println!("First message content: {}", first_message.content);
                        if let Some(user_id) = extract_user_id_from_message(first_message) {
                            new_state.user_to_thread.insert(user_id, thread.id);
                            new_state.thread_to_user.insert(thread.id, user_id);
                            println!(
                                "Successfully extracted user ID: {} for thread: {}",
                                user_id, thread.id
                            );
                        } else {
                            println!(
                                "Could not extract user ID from thread: {}. Message content: {}",
                                thread.id, first_message.content
                            );
                        }
                    } else {
                        println!("No messages found in thread: {}", thread.id);
                    }
                } else {
                    println!("Failed to fetch messages for thread: {}", thread.id);
                }
            }
        }

        let mut state_guard = state.lock().await;
        *state_guard = new_state;

        println!(
            "State resynced. Active threads: {}",
            state_guard.user_to_thread.len()
        );

        Ok(())
    } else {
        Err("Could not find the specified forum channel".to_string())
    }
}

fn extract_user_id_from_message(message: &Message) -> Option<UserId> {
    message
        .content
        .split_whitespace()
        .find_map(|word| {
            if word.starts_with("<@") && word.ends_with('>') {
                word.trim_start_matches("<@")
                    .trim_end_matches('>')
                    .parse::<u64>()
                    .ok()
                    .map(UserId::new)
            } else {
                None
            }
        })
        .or_else(|| {
            message
                .content
                .rsplit_once("(ID: ")
                .and_then(|(_, id_part)| id_part.split_once(')'))
                .and_then(|(id_str, _)| id_str.trim().parse::<u64>().ok())
                .map(UserId::new)
        })
}
