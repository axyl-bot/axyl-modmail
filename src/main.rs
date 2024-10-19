mod bot;
mod commands;
mod config;

use bot::run_bot;

#[tokio::main]
async fn main() {
    if let Err(e) = run_bot().await {
        eprintln!("Error: {}", e);
    }
}
