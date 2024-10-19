use crate::commands::*;
use crate::config::Config;
use serenity::all::*;
use serenity::async_trait;
use serenity::builder::CreateInteractionResponse;
use serenity::model::gateway::Ready;
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

        let guild_id = Config::get().guild_id;

        let commands = GuildId::new(guild_id)
            .set_commands(
                &ctx.http,
                vec![CreateCommand::new("modmail")
                    .description("Send a modmail")
                    .add_option(
                        CreateCommandOption::new(
                            CommandOptionType::String,
                            "message",
                            "The message to send as modmail",
                        )
                        .required(true),
                    )],
            )
            .await;

        println!("Slash commands registered: {:#?}", commands);
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
