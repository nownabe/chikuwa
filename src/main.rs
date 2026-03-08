mod agent;
mod app;
mod event;
mod git;
mod hook;
mod ipc;
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
    /// Hook mode: update agent state from Claude Code hooks (reads event from stdin JSON)
    Hook,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook) => {
            hook::run().await?;
        }
        None => {
            app::run().await?;
        }
    }

    Ok(())
}
