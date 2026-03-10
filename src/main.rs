mod agent;
mod app;
mod event;
mod git;
mod hook;
mod ipc;
mod tmux;
mod ui;
mod usage;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chikuwa", about = "tmux AI Agent monitor TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Store all received hook events to a JSONL file for debugging
    #[arg(long)]
    store_events: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Hook mode: update agent state from Claude Code hooks (reads event from stdin JSON)
    Hook,
    /// Notify the TUI of a tmux change (used by tmux hooks)
    Notify,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Hook) => {
            hook::run().await?;
        }
        Some(Commands::Notify) => {
            ipc::send_notify().await?;
        }
        None => {
            app::run(cli.store_events).await?;
        }
    }

    Ok(())
}
