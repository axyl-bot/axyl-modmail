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
    state: Arc<Mutex<ModmailState>>,
) -> Result<ChannelId, String> {
    let config = Config::get();
    let forum_channel_id = config.forum_channel_id;
    let role_id = config.role_id;

    let existing_thread = {
        let state_guard = state.lock().await;
        state_guard.user_to_thread.get(&user.id).cloned()
    };

    if let Some(thread_id) = existing_thread {
        if thread_id.to_channel(&ctx.http).await.is_ok() {
            return Ok(thread_id);
        }
    }

    let forum_channel = ChannelId::new(forum_channel_id)
        .to_channel(&ctx.http)
        .await
        .map_err(|_| "Could not find the specified forum channel".to_string())?;

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
                        "<@&{}> New modmail from {} (ID: {})",
                        role_id,
                        user.mention(),
                        user.id
                    )),
                ),
            )
            .await
            .map_err(|why| format!("Error creating modmail thread: {}", why))?;

        if let Ok(messages) = thread
            .messages(&ctx.http, GetMessages::default().limit(1))
            .await
        {
            if let Some(first_message) = messages.first() {
                let _ = first_message.pin(&ctx.http).await;
            }
        }

        let mut state = state.lock().await;
        state.user_to_thread.insert(user.id, thread.id);
        state.thread_to_user.insert(thread.id, user.id);
        Ok(thread.id)
    } else {
        Err("Could not find the specified forum channel".to_string())
    }
}

pub async fn modmail(
    ctx: &Context,
    command: &CommandInteraction,
    state: Arc<Mutex<ModmailState>>,
) -> String {
    let content = command
        .data
        .options
        .get(0)
        .and_then(|opt| opt.value.as_str())
        .unwrap_or("No content provided")
        .to_string();

    let thread_id = {
        let state_guard = state.lock().await;
        state_guard.user_to_thread.get(&command.user.id).cloned()
    };

    let thread_id = if let Some(thread_id) = thread_id {
        match thread_id.to_channel(&ctx.http).await {
            Ok(_) => thread_id,
            Err(_) => match create_modmail_thread(ctx, &command.user, state.clone()).await {
                Ok(new_thread_id) => new_thread_id,
                Err(why) => return format!("Error creating modmail thread: {}", why),
            },
        }
    } else {
        match create_modmail_thread(ctx, &command.user, state.clone()).await {
            Ok(new_thread_id) => new_thread_id,
            Err(why) => return format!("Error creating modmail thread: {}", why),
        }
    };

    let formatted_message = format!("(**User**) {}: **{}**", command.user.mention(), content);
    if let Err(why) = thread_id.say(&ctx.http, &formatted_message).await {
        println!("Error sending message to thread: {:?}", why);
        return "Error sending message to modmail thread.".to_string();
    }

    "Modmail sent successfully! You can now continue the conversation in DMs.".to_string()
}

pub async fn handle_dm(ctx: &Context, msg: &Message, state: Arc<Mutex<ModmailState>>) {
    if msg.author.id == ctx.http.get_current_user().await.unwrap().id {
        return;
    }

    let thread_id = {
        let state_guard = state.lock().await;
        state_guard.user_to_thread.get(&msg.author.id).cloned()
    };

    let thread_id = if let Some(thread_id) = thread_id {
        match thread_id.to_channel(&ctx.http).await {
            Ok(_) => thread_id,
            Err(_) => match create_modmail_thread(ctx, &msg.author, state.clone()).await {
                Ok(new_thread_id) => new_thread_id,
                Err(why) => {
                    println!("Error creating new modmail thread: {}", why);
                    return;
                }
            },
        }
    } else {
        match create_modmail_thread(ctx, &msg.author, state.clone()).await {
            Ok(new_thread_id) => new_thread_id,
            Err(why) => {
                println!("Error creating new modmail thread: {}", why);
                return;
            }
        }
    };

    let content = if msg.content.is_empty() && !msg.attachments.is_empty() {
        "Sent an attachment".to_string()
    } else {
        msg.content.clone()
    };

    let formatted_message = format!("(**User**) {}: **{}**", msg.author.mention(), content);

    let mut message_builder = CreateMessage::new().content(formatted_message);

    for attachment in &msg.attachments {
        message_builder = message_builder.add_file(
            CreateAttachment::url(&ctx.http, &attachment.url)
                .await
                .unwrap(),
        );
    }

    if let Err(why) = thread_id.send_message(&ctx.http, message_builder).await {
        println!("Error sending message to thread: {:?}", why);
    }

    let confirmation_message = "Your message has been received and forwarded to the staff. They will respond as soon as possible.";
    if let Err(why) = msg.channel_id.say(&ctx.http, confirmation_message).await {
        println!("Error sending confirmation message to user: {:?}", why);
    }
}

pub async fn handle_thread_message(ctx: &Context, msg: &Message, state: Arc<Mutex<ModmailState>>) {
    if msg.author.id == ctx.http.get_current_user().await.unwrap().id {
        return;
    }

    let state = state.lock().await;
    if let Some(&user_id) = state.thread_to_user.get(&msg.channel_id) {
        let dm_result = user_id.create_dm_channel(&ctx.http).await;

        match dm_result {
            Ok(channel) => {
                let content = if msg.content.is_empty() && !msg.attachments.is_empty() {
                    "Sent an attachment".to_string()
                } else {
                    msg.content.clone()
                };

                let formatted_content = format!("(**Staff**) {}: **{}**", msg.author.name, content);

                let mut message_builder = CreateMessage::new().content(formatted_content);

                for attachment in &msg.attachments {
                    message_builder = message_builder.add_file(
                        CreateAttachment::url(&ctx.http, &attachment.url)
                            .await
                            .unwrap(),
                    );
                }

                if let Err(why) = channel.send_message(&ctx.http, message_builder).await {
                    println!("Error sending DM: {:?}", why);
                    let error_message =
                        "Failed to send the message to the user. They may have DMs disabled.";
                    if let Err(why) = msg.channel_id.say(&ctx.http, error_message).await {
                        println!("Error sending error message to staff: {:?}", why);
                    }
                } else {
                    let confirmation_message = "Your message has been sent to the user.";
                    if let Err(why) = msg.channel_id.say(&ctx.http, confirmation_message).await {
                        println!("Error sending confirmation message to staff: {:?}", why);
                    }
                }
            }
            Err(_) => {
                let error_message = "Unable to send a message to this user. They may have DMs disabled or have blocked the bot.";
                if let Err(why) = msg.channel_id.say(&ctx.http, error_message).await {
                    println!("Error sending error message to staff: {:?}", why);
                }
            }
        }
    }
}

pub async fn close_thread(
    ctx: &Context,
    command: &CommandInteraction,
    state: Arc<Mutex<ModmailState>>,
) -> String {
    let thread_id = command.channel_id;

    let user_id = {
        let state_guard = state.lock().await;
        state_guard.thread_to_user.get(&thread_id).cloned()
    };

    if let Some(user_id) = user_id {
        if let Err(why) = thread_id.delete(&ctx.http).await {
            return format!("Failed to delete the thread: {}", why);
        }

        {
            let mut state_guard = state.lock().await;
            state_guard.thread_to_user.remove(&thread_id);
            state_guard.user_to_thread.remove(&user_id);
        }

        if let Ok(dm_channel) = user_id.create_dm_channel(&ctx.http).await {
            let _ = dm_channel.say(&ctx.http, "Your modmail thread has been closed by a staff member. If you need further assistance, feel free to start a new modmail.").await;
        }

        "Thread closed successfully. The user has been notified.".to_string()
    } else {
        "This command can only be used in a modmail thread.".to_string()
    }
}
