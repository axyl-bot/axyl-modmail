use crate::config::Config;
use serenity::all::*;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ModmailState {
    pub user_to_thread: std::collections::HashMap<UserId, ChannelId>,
    pub thread_to_user: std::collections::HashMap<ChannelId, UserId>,
}

pub async fn create_modmail_thread(
    ctx: &Context,
    user: &User,
    content: &str,
    state: Arc<Mutex<ModmailState>>,
) -> Result<ChannelId, String> {
    let config = Config::get();
    let forum_channel_id = config.forum_channel_id;
    let role_id = config.role_id;

    let forum_channel = match ChannelId::new(forum_channel_id).to_channel(&ctx.http).await {
        Ok(channel) => channel,
        Err(_) => return Err("Could not find the specified forum channel".to_string()),
    };

    if let Channel::Guild(channel) = forum_channel {
        if channel.kind != ChannelType::Forum {
            return Err("The specified channel is not a forum channel".to_string());
        }

        let thread = channel
            .create_forum_post(
                &ctx.http,
                CreateForumPost::new(
                    format!("Modmail from {}", user.name),
                    CreateMessage::new().content(format!(
                        "<@&{}> New modmail from {}\n\n{}: {}",
                        role_id,
                        user.mention(),
                        user.mention(),
                        content
                    )),
                ),
            )
            .await;

        match thread {
            Ok(thread) => {
                let mut state = state.lock().await;
                state.user_to_thread.insert(user.id, thread.id);
                state.thread_to_user.insert(thread.id, user.id);
                Ok(thread.id)
            }
            Err(why) => Err(format!("Error creating modmail thread: {}", why)),
        }
    } else {
        Err("Could not find the specified forum channel".to_string())
    }
}

pub async fn modmail(
    ctx: &Context,
    command: &CommandInteraction,
    state: Arc<Mutex<ModmailState>>,
) -> String {
    let content = if let Some(option) = command.data.options.get(0) {
        option
            .value
            .as_str()
            .unwrap_or("No content provided")
            .to_string()
    } else {
        "No content provided".to_string()
    };

    match create_modmail_thread(ctx, &command.user, &content, state).await {
        Ok(_) => "Modmail sent successfully! You can now continue the conversation in DMs.".to_string(),
        Err(why) => format!("Error sending modmail: {}", why),
    }
}

pub async fn handle_dm(ctx: &Context, msg: &Message, state: Arc<Mutex<ModmailState>>) {
    if msg.author.id == ctx.http.get_current_user().await.unwrap().id {
        return;
    }

    let thread_id = {
        let state_guard = state.lock().await;
        if let Some(&thread_id) = state_guard.user_to_thread.get(&msg.author.id) {
            Some(thread_id)
        } else {
            None
        }
    };

    let thread_id = if let Some(thread_id) = thread_id {
        thread_id
    } else {
        match create_modmail_thread(ctx, &msg.author, &msg.content, state.clone()).await {
            Ok(thread_id) => thread_id,
            Err(why) => {
                println!("Error creating modmail thread: {}", why);
                return;
            }
        }
    };

    let formatted_message = format!("{}: {}", msg.author.mention(), msg.content);
    if let Err(why) = thread_id.say(&ctx.http, &formatted_message).await {
        println!("Error sending message to thread: {:?}", why);
    }
}

pub async fn handle_thread_message(ctx: &Context, msg: &Message, state: Arc<Mutex<ModmailState>>) {
    if msg.author.id == ctx.http.get_current_user().await.unwrap().id {
        return;
    }

    let state = state.lock().await;
    if let Some(&user_id) = state.thread_to_user.get(&msg.channel_id) {
        if let Ok(channel) = user_id.create_dm_channel(&ctx.http).await {
            let formatted_message = format!("{}: {}", msg.author.name, msg.content);
            if let Err(why) = channel.say(&ctx.http, &formatted_message).await {
                println!("Error sending DM: {:?}", why);
            }
        }
    }
}
