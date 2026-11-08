//! CLI startup; all reusable configuration behavior lives in rit-core.
#![forbid(unsafe_code)]
mod cli;
mod output;

use clap::Parser;
use cli::{
    CheckpointCommand, Cli, Command, ConfigCommand, DependenciesCommand, McpCommand,
    RepositoryCommand, StorageCommand,
};
use rit_core::{
    config::{self, ConfigFile, Overrides},
    error::ConfigError,
};
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

// Clap owns usage exit code 2. Configuration is 3, environment/runtime is 4,
// and internal failures are 5. Successful commands return 0.
const CONFIG_FAILURE: u8 = 3;
const RUNTIME_FAILURE: u8 = 4;
const INTERNAL_FAILURE: u8 = 5;

fn env_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Environment { name }),
    }
}

fn default_file() -> Option<ConfigFile> {
    // XDG's relative config paths are invalid; fall back to HOME, not cwd.
    #[cfg(not(windows))]
    let base: Option<OsString> = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| PathBuf::from(v).is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".config").into_os_string())
        });
    #[cfg(windows)]
    let base: Option<OsString> = std::env::var_os("APPDATA");
    base.map(|base| ConfigFile {
        path: PathBuf::from(base).join("rit").join("config.toml"),
        required: false,
    })
}

fn run(cli: Cli) -> Result<(), (u8, String)> {
    let file = cli
        .config
        .as_ref()
        .map(|p| ConfigFile {
            path: p.clone(),
            required: true,
        })
        .or_else(default_file);
    let environment = Overrides {
        log_level: env_value("RIT_LOG_LEVEL").map_err(|e| (CONFIG_FAILURE, e.to_string()))?,
        log_format: env_value("RIT_LOG_FORMAT").map_err(|e| (CONFIG_FAILURE, e.to_string()))?,
    };
    let overrides = Overrides {
        log_level: cli.log_level.clone(),
        log_format: cli.log_format.clone(),
    };
    let resolved = config::resolve(file.as_ref(), &environment, &overrides).map_err(|e| {
        (
            CONFIG_FAILURE,
            format!("configuration failed: {e}. Check the selected configuration and overrides."),
        )
    })?;
    output::init_logging(&resolved.config).map_err(|e| (INTERNAL_FAILURE, e))?;
    tracing::debug!("configuration validated; executing command");
    let git_arg = cli.git_executable.as_deref().map(|v| v.as_os_str());
    match cli.command {
        Command::Config {
            command: ConfigCommand::Validate,
        } => output::config_valid(resolved.config.schema_version).map_err(|e| (RUNTIME_FAILURE, e)),
        Command::Doctor { json } => output::doctor(
            &resolved,
            json,
            cli.git_executable.as_deref(),
            cli.data_dir.as_deref(),
        )
        .map_err(|e| (RUNTIME_FAILURE, e)),
        Command::Storage { command } => {
            let data = rit_storage::data_dir(cli.data_dir.as_deref())
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            match command {
                StorageCommand::Verify { json } => {
                    let result =
                        rit_storage::verify(&data).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
                    let info=result.map(|r| serde_json::json!({"application_id":r.application_id,"user_version":r.user_version,"marked_rit":r.marked_rit,"valid_legacy":r.valid_legacy}));
                    output::value(
                        &serde_json::json!({"schema_version":1,"storage":info}),
                        json,
                    )
                    .map_err(|e| (RUNTIME_FAILURE, e))
                }
                StorageCommand::AdoptLegacy { .. } => Err((RUNTIME_FAILURE, "operation unavailable: legacy adoption requires a verified identity-bound intent and SQLite-consistent backup".into())),
            }
        }
        Command::Mcp {
            command: McpCommand::Serve { repo },
        } => {
            let scope = repo
                .as_deref()
                .map(rit_protocol::ReadOnlyScope::open)
                .transpose()
                .map_err(|e| (RUNTIME_FAILURE, e))?;
            rit_protocol::serve_stdio(scope).map_err(|e| (RUNTIME_FAILURE, e.to_string()))
        }
        Command::Dependencies { command } => {
            let (repo, json) = match command {
                DependenciesCommand::Inspect { repo, json }
                | DependenciesCommand::Refresh { repo, json } => (repo, json),
            };
            let service = rit_application::GitReadService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let graph = service
                .dependencies(&repo)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(&serde_json::json!({"schema_version":1,"graph":graph}), json)
                .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Impact { repo, files, json } => {
            let service = rit_application::GitReadService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let graph = service
                .dependencies(&repo)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let domain = rit_core::impact::DependencyGraph {
                source_revision: graph.commit.to_string(),
                edges: graph
                    .edges
                    .iter()
                    .map(|e| rit_core::impact::DependencyEdge {
                        dependent: e.dependent.clone(),
                        dependency: e.dependency.clone(),
                        evidence: format!("{}:{}:{}", e.dependent, e.line, e.specifier),
                    })
                    .collect(),
            };
            let reports=files.iter().map(|file| {let impact=domain.impact(file,&graph.commit.to_string());serde_json::json!({"changed":file,"known_dependents":impact.known_dependents,"complete":false})}).collect::<Vec<_>>();
            output::value(&serde_json::json!({"schema_version":1,"commit":graph.commit,"reports":reports,"unresolved":graph.unresolved,"problematic_files":graph.problematic_files,"omitted_non_regular":graph.omitted_non_regular,"complete":false,"scope":graph.scope}),json).map_err(|e|(RUNTIME_FAILURE,e))
        }
        Command::Checkpoint {
            command:
                CheckpointCommand::Create {
                    repo,
                    commit_only,
                    json,
                },
        } => {
            if !commit_only {
                return Err((RUNTIME_FAILURE,"operation unavailable: full checkpoint capture is not implemented; --commit-only preserves committed HEAD only".into()));
            }
            let service = rit_application::GitCheckpointService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let item = service
                .create_commit_only(&repo)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"checkpoint":item}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Checkpoint {
            command: CheckpointCommand::List { repo, json },
        } => {
            let service = rit_application::GitCheckpointService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let items = service
                .list(&repo)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"checkpoints":items}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Checkpoint {
            command: CheckpointCommand::Verify { repo, id, json },
        } => {
            let id = id
                .parse()
                .map_err(|_| (RUNTIME_FAILURE, "invalid checkpoint ID".into()))?;
            let service = rit_application::GitCheckpointService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let item = service
                .verify(&repo, id)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(&serde_json::json!({"schema_version":1,"checkpoint":item,"git_fsck":"passed","uncommitted_work_captured":false}),json).map_err(|e|(RUNTIME_FAILURE,e))
        }
        Command::Task { .. }
        | Command::Submission { .. }
        | Command::Checkpoint { .. }
        | Command::Workspace { .. }
        | Command::Agent { .. }
        | Command::Identity { .. }
        | Command::Whoami
        | Command::Permission { .. }
        | Command::Policy { .. }
        | Command::Audit { .. } => Err((
            RUNTIME_FAILURE,
            rit_application::contracts::CapabilityError::StorageUnverified.to_string(),
        )),
        Command::Inspect { path, json } => {
            let service = rit_application::GitReadService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let ins = service
                .inspect(&path)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"inspection":ins}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Repository {
            command:
                command @ (RepositoryCommand::Inspect { .. }
                | RepositoryCommand::Status { .. }
                | RepositoryCommand::Branches { .. }
                | RepositoryCommand::Tags { .. }
                | RepositoryCommand::Refs { .. }
                | RepositoryCommand::Log { .. }
                | RepositoryCommand::Worktrees { .. }
                | RepositoryCommand::Diff { .. }
                | RepositoryCommand::Doctor { .. }),
        } => {
            let service = rit_application::GitReadService::new(
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
            );
            let fail = |e: rit_application::ApplicationError| (RUNTIME_FAILURE, e.to_string());
            let (result, json) = match command {
                RepositoryCommand::Inspect { path, json } => (
                    serde_json::json!({"schema_version":1,"inspection":service.inspect(&path).map_err(fail)?}),
                    json,
                ),
                RepositoryCommand::Doctor { path, json } => (
                    serde_json::json!({"schema_version":1,"inspection":service.inspect(&path).map_err(fail)?,"security_gate":"blocked_phase_2_1","health":"validation_pending"}),
                    json,
                ),
                RepositoryCommand::Status { path, json } => (
                    serde_json::json!({"schema_version":1,"status":service.status(&path).map_err(fail)?}),
                    json,
                ),
                RepositoryCommand::Worktrees { path, json } => (
                    serde_json::json!({"schema_version":1,"worktrees":service.worktrees(&path).map_err(fail)?}),
                    json,
                ),
                RepositoryCommand::Diff { path, json } => (
                    serde_json::json!({"schema_version":1,"diff":service.diff(&path).map_err(fail)?}),
                    json,
                ),
                RepositoryCommand::Branches { path, json } => (
                    serde_json::json!({"schema_version":1,"references":service.references(&path).map_err(fail)?.into_iter().filter(|r|r.name.starts_with("refs/heads/")||r.name.starts_with("refs/remotes/")).collect::<Vec<_>>()}),
                    json,
                ),
                RepositoryCommand::Tags { path, json } => (
                    serde_json::json!({"schema_version":1,"references":service.references(&path).map_err(fail)?.into_iter().filter(|r|r.name.starts_with("refs/tags/")).collect::<Vec<_>>()}),
                    json,
                ),
                RepositoryCommand::Refs { path, json } => (
                    serde_json::json!({"schema_version":1,"references":service.references(&path).map_err(fail)?}),
                    json,
                ),
                RepositoryCommand::Log { path, limit, json } => (
                    serde_json::json!({"schema_version":1,"commits":service.history(&path,limit).map_err(fail)?}),
                    json,
                ),
                _ => {
                    return Err((
                        INTERNAL_FAILURE,
                        "unexpected read-only repository command".into(),
                    ));
                }
            };
            output::value(&result, json).map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Repository {
            command: RepositoryCommand::List { json },
        } => {
            let data = rit_storage::data_dir(cli.data_dir.as_deref())
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let repos = match rit_storage::Registry::open_readonly(&data)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?
            {
                Some(registry) => registry
                    .list()
                    .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?,
                None => Vec::new(),
            };
            output::value(
                &serde_json::json!({"schema_version":1,"repositories":rit_application::annotate_locations(repos)}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        command => {
            let git =
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let data = rit_storage::data_dir(cli.data_dir.as_deref())
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let services = rit_application::Repositories::new(git, data);
            let fail = |e: rit_application::ApplicationError| (RUNTIME_FAILURE, e.to_string());
            match command {
                Command::Init {path,json} => { let (repo,created)=services.register(&path,None).map_err(fail)?; output::value(&serde_json::json!({"schema_version":1,"created":created,"repository":repo}),json) },
                Command::Repository {command:RepositoryCommand::Register{path,name,json}} => {let (repo,created)=services.register(&path,name.as_deref()).map_err(fail)?; output::value(&serde_json::json!({"schema_version":1,"created":created,"repository":repo}),json)},
                Command::Repository {command:RepositoryCommand::Info{target,json}} => {let (repo,ins)=services.live(&target).map_err(fail)?;output::value(&serde_json::json!({"schema_version":1,"repository":repo,"inspection":ins}),json)},
                Command::Status {target,json}=>{let (_,ins)=services.live(&target).map_err(fail)?;let status=ins.status.ok_or_else(||(RUNTIME_FAILURE,"bare repositories have no working-tree status".to_string()))?;output::value(&serde_json::json!({"schema_version":1,"status":status}),json)},
                _=>Err("unexpected command".to_string()),
            }.map_err(|e| (RUNTIME_FAILURE,e))
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("rit: {message}");
            ExitCode::from(code)
        }
    }
}
