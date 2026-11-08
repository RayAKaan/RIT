use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rit",
    version,
    about = "Local-first coordination for Git repositories (foundation preview)",
    after_help = "Examples:
  rit doctor --json
  rit --config ./rit.toml config validate"
)]
pub struct Cli {
    /// Read a specific configuration file (must exist).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Override the log level: error, warn, info, debug, trace.
    #[arg(long, global = true, value_name = "LEVEL")]
    pub log_level: Option<String>,
    /// Override the log format: text or json.
    #[arg(long, global = true, value_name = "FORMAT")]
    pub log_format: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the Phase 1 runtime and configuration.
    Doctor {
        /// Print a JSON diagnostic report to stdout.
        #[arg(long)]
        json: bool,
    },
    /// Inspect the configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate the effective configuration without modifying files.
    Validate,
}
