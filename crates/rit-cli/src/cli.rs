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
    /// Credential file reserved for future authenticated commands; never pass a token.
    #[arg(long, global = true, value_name = "PATH")]
    pub credential_file: Option<PathBuf>,
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
    /// Inspect or explicitly adopt a legacy local registry.
    Storage {
        #[command(subcommand)]
        command: StorageCommand,
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
    /// Manage registered repositories and inspect Git references.
    #[command(alias = "repo")]
    Repository {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    /// Task and review operations (currently unavailable).
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Submission and integration operations (currently unavailable).
    Submission {
        #[command(subcommand)]
        command: SubmissionCommand,
    },
    /// Checkpoint operations (currently unavailable).
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
    /// Workspace operations (currently unavailable).
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Agent execution operations (currently unavailable).
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Identity operations (currently unavailable).
    Identity {
        #[command(subcommand)]
        command: IdentityCommand,
    },
    /// Show the currently authenticated principal (currently unavailable).
    Whoami,
    /// Permission operations (currently unavailable).
    Permission {
        #[command(subcommand)]
        command: PermissionCommand,
    },
    /// Show security policy (currently unavailable).
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    /// Audit operations (currently unavailable).
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
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
    /// List branches and remote-tracking branches directly from Git; no registry required.
    Branches {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List local branches, tags and remote-tracking references directly from Git.
    Refs {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect recent HEAD history without messages or contents; no registry required.
    Log {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
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

#[derive(Debug, Subcommand)]
pub enum StorageCommand {
    /// Inspect application ID and legacy schema without modifying the database.
    Verify {
        #[arg(long)]
        json: bool,
    },
    /// Adopt an exact unmarked Phase 2 schema after taking a backup.
    AdoptLegacy {
        /// Confirm that this database is yours and was backed up.
        #[arg(long)]
        confirm: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum IdentityCommand {
    /// Explicit owner bootstrap; disabled.
    Bootstrap,
    /// Create a human principal; disabled.
    CreateHuman { name: String },
    /// Create an agent principal; disabled.
    CreateAgent { name: String },
    /// List principals; disabled.
    List,
    /// Inspect a principal; disabled.
    Info { id: String },
    /// Suspend a principal; disabled.
    Suspend { id: String },
    /// Reactivate a principal; disabled.
    Reactivate { id: String },
    /// Revoke a principal; disabled.
    Revoke { id: String },
    /// Credential lifecycle; disabled.
    Credential {
        #[command(subcommand)]
        command: CredentialCommand,
    },
}
#[derive(Debug, Subcommand)]
pub enum CredentialCommand {
    /// Issue a credential into a secure file; disabled.
    Issue { id: String },
    /// Rotate a credential; disabled.
    Rotate { id: String },
    /// Revoke a credential; disabled.
    Revoke { id: String },
}
#[derive(Debug, Subcommand)]
pub enum PermissionCommand {
    /// List role definitions; disabled pending authenticated access.
    Roles,
    /// Grant a role; disabled.
    Grant { principal: String, role: String },
    /// Revoke a role; disabled.
    Revoke { principal: String, role: String },
    /// List assignments; disabled.
    List,
    /// Check a capability; disabled.
    Check { capability: String },
}
#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Show policy; disabled.
    Show,
}
#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// List events; disabled.
    List,
    /// Verify audit chain; disabled.
    Verify,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Create a workspace; disabled.
    Create { repository: String },
    /// List workspaces; disabled.
    List,
    /// Inspect a workspace; disabled.
    Status { id: String },
    /// Plan workspace removal; disabled.
    Remove { id: String },
}
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Run an agent only in a validated isolated environment; disabled.
    Run { workspace: String },
    /// Inspect agent activity; disabled.
    Activity { id: String },
}

#[derive(Debug, Subcommand)]
pub enum CheckpointCommand {
    /// Capture; disabled.
    Create { workspace: String },
    /// List; disabled.
    List { workspace: String },
    /// Inspect metadata; disabled.
    Show { id: String },
    /// Preview partial restore; disabled.
    Preview { id: String },
    /// Restore; disabled.
    Restore { id: String },
}

#[derive(Debug, Subcommand)]
pub enum SubmissionCommand {
    /// Create a submission; disabled.
    Create { workspace: String },
    /// List submissions; disabled.
    List,
    /// Preview integration; disabled.
    Preview { id: String },
    /// Publish a validated candidate; disabled.
    Publish { id: String },
}

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Create a task; disabled.
    Create { repository: String },
    /// List tasks; disabled.
    List,
    /// Inspect a task; disabled.
    Status { id: String },
    /// Record a review; disabled.
    Review { submission: String },
}
