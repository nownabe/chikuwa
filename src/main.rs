mod agent;
mod app;
mod event;
mod hook;
mod tmux;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chikuwa", about = "tmux AI Agent monitor TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Hook mode: update agent state from Claude Code hooks
    Hook {
        /// Event type: started, running, waiting, permission, notification, ended, statusline
        event: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook { event }) => {
            hook::run(&event)?;
        }
        None => {
            app::run().await?;
        }
    }

    Ok(())
}
