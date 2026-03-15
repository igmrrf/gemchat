use clap::{Parser, Subcommand};

/// Gemchat — Multi-Agent AI coding CLI powered by Gemini
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Multi-agent AI coding assistant powered by Gemini",
    long_about = None
)]
pub struct Cli {
    /// Model to use (e.g. gemini-3-flash-preview, gemini-2.5-pro)
    #[arg(short, long, default_value = "gemini-3-flash-preview")]
    pub model: String,

    /// Working directory (defaults to current dir)
    #[arg(short = 'd', long)]
    pub working_dir: Option<String>,

    /// Log level: error, warn, info, debug, trace
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start interactive TUI chat (default)
    Chat {
        /// Initial prompt to send on startup
        #[arg(short, long)]
        prompt: Option<String>,
    },

    /// Run multi-agent pipeline on a task
    Agent {
        /// The task description
        task: String,

        /// Enable auto-approval for all tool calls
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
    },

    /// Show current config
    Config,

    /// Initialize a new project config (.gemchat.toml)
    Init,
}
