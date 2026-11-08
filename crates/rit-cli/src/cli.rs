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
    /// Run a local stdio protocol server with optional read-only repository scope.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Reconcile live Git activity without persistent event history.
    Activity {
        #[command(subcommand)]
        command: ActivityCommand,
    },
    /// Simulate a Git merge without changing source/target refs or worktrees.
    Conflicts {
        #[command(subcommand)]
        command: ConflictsCommand,
    },
    /// Analyze committed source dependencies without registry access.
    Dependencies {
        #[command(subcommand)]
        command: DependenciesCommand,
    },
    /// Analyze known dependents in committed HEAD (partial analysis).
    Impact {
        #[arg(long)]
        repo: PathBuf,
        /// Analyze the specified commit rather than HEAD.
        #[arg(long, conflicts_with_all = ["branch", "worktree", "workspace"])]
        commit: Option<String>,
        /// Analyze the specified branch rather than HEAD.
        #[arg(long, conflicts_with_all = ["commit", "worktree", "workspace"])]
        branch: Option<String>,
        /// Analyze non-atomic tracked and untracked worktree source bytes.
        #[arg(long, conflicts_with_all=["commit","branch","workspace"])]
        worktree: bool,
        /// Analyze an identified RIT-managed worktree.
        #[arg(long, conflicts_with_all=["commit","branch","worktree"])]
        workspace: Option<String>,
        #[arg(long)]
        files: Vec<String>,
        #[arg(long)]
        json: bool,
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
    /// Bounded Git-object content and commit-only checkpoints; restore unavailable.
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
    /// Git-native cooperative workspace operations; not an isolation boundary.
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
    /// Inspect Git by explicit path without registry access.
    Inspect {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect worktree status by explicit path without registry access.
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect lightweight and annotated tags.
    Tags {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect Git worktree inventory.
    Worktrees {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect tracked staged and unstaged file differences.
    Diff {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect repository health via Git's read-only discovery.
    Doctor {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
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
    /// Adopt a legacy schema (currently unavailable; backup and crash safety are not verified).
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
    /// Create a Git worktree and a Git-ref-backed coordination manifest.
    Create {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long)]
        json: bool,
    },
    /// List managed workspace observations.
    List {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Reconcile manifests and branch/worktree inventory.
    Reconcile {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a managed workspace.
    Inspect {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a managed workspace and its live status.
    Status {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Lock its actual Git worktree.
    Lock {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    /// Unlock its actual Git worktree.
    Unlock {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Removal is disabled until non-destructive handling is verified.
    Remove {
        id: String,
        #[arg(long)]
        repo: PathBuf,
    },
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
    /// Create an explicitly limited committed-HEAD reference; no uncommitted work is captured.
    Create {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        commit_only: bool,
        /// Capture bounded raw workspace and index content.
        #[arg(long, conflicts_with = "commit_only")]
        workspace: Option<String>,
        /// Explicitly include nonignored untracked files (may contain secrets).
        #[arg(long)]
        include_untracked: bool,
        /// Explicitly capture an ignored file by exact relative path (repeatable).
        #[arg(long)]
        include_ignored: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// List commit-only checkpoint refs.
    List {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a commit-only checkpoint with Git fsck.
    Verify {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a content checkpoint manifest.
    Inspect {
        id: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show metadata; disabled.
    Show { id: String },
    /// Preview full restore; disabled.
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

#[derive(Debug, Subcommand)]
pub enum DependenciesCommand {
    /// Inspect static imports in committed HEAD; no durable cache.
    Inspect {
        #[arg(long)]
        repo: PathBuf,
        /// Analyze this commit instead of HEAD.
        #[arg(long, conflicts_with_all = ["branch", "worktree", "workspace"])]
        commit: Option<String>,
        /// Analyze this branch instead of HEAD.
        #[arg(long, conflicts_with_all = ["commit", "worktree", "workspace"])]
        branch: Option<String>,
        /// Analyze non-atomic tracked and untracked worktree source bytes.
        #[arg(long, conflicts_with_all=["commit","branch","workspace"])]
        worktree: bool,
        /// Analyze an identified RIT-managed worktree.
        #[arg(long, conflicts_with_all=["commit","branch","worktree"])]
        workspace: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Recompute static imports in committed HEAD; no durable cache.
    Refresh {
        #[arg(long)]
        repo: PathBuf,
        /// Analyze this commit instead of HEAD.
        #[arg(long, conflicts_with_all = ["branch", "worktree", "workspace"])]
        commit: Option<String>,
        /// Analyze this branch instead of HEAD.
        #[arg(long, conflicts_with_all = ["commit", "worktree", "workspace"])]
        branch: Option<String>,
        /// Analyze non-atomic tracked and untracked worktree source bytes.
        #[arg(long, conflicts_with_all=["commit","branch","workspace"])]
        worktree: bool,
        /// Analyze an identified RIT-managed worktree.
        #[arg(long, conflicts_with_all=["commit","branch","worktree"])]
        workspace: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Serve on stdio; --repo explicitly exposes limited read-only repository data.
    Serve {
        #[arg(long)]
        repo: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ActivityCommand {
    /// Reconcile current Git worktree state once.
    Reconcile {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List current Git worktree observations (not historical events).
    List {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Report exact-path overlap, not necessarily conflicts.
    Overlaps {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Watch native notifications and periodically reconcile Git; events can be missed.
    Watch {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
        /// Explicit legacy snapshot polling count; omit to use native notifications.
        #[arg(long)]
        count: Option<u32>,
        /// Stop notification mode after this many milliseconds (omit for continuous watch).
        #[arg(long)]
        duration_ms: Option<u64>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConflictsCommand {
    /// Run an ephemeral Git-native merge simulation with stage and structural evidence.
    Simulate {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        left: String,
        #[arg(long)]
        right: String,
        #[arg(long)]
        json: bool,
    },
}
