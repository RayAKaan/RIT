use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rit",
    version,
    about = "Local-first coordination for Git repositories (development preview)",
    after_help = "Examples:\n  rit doctor --json\n  rit inspect . --json\n  rit repository register ."
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
    /// RIT's local data directory, outside managed Git repositories.
    #[arg(long, global = true, value_name = "PATH")]
    pub data_dir: Option<PathBuf>,
    /// Git executable path or name; overrides RIT_GIT_EXECUTABLE.
    #[arg(long, global = true, value_name = "PATH")]
    pub git_executable: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check runtime, Git and registry health.
    Doctor {
        /// Output machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Inspect the configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Register an existing Git repository; does not run git init.
    Init {
        /// Repository directory (default current directory).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Manage registered repositories.
    Repository {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    /// Inspect an existing Git repository without registering it.
    Inspect {
        /// Repository directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect live working-tree status by ID or path.
    Status {
        /// Registered repository ID or path.
        #[arg(default_value = ".")]
        target: String,
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum RepositoryCommand {
    /// Register an existing Git repository.
    Register {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List registered repositories.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Inspect a registered repository by ID or path.
    Info {
        #[arg(default_value = ".")]
        target: String,
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate the effective configuration without modifying files.
    Validate,
}
