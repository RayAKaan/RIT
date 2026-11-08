//! CLI startup; all reusable configuration behavior lives in rit-core.
#![forbid(unsafe_code)]
mod cli;
mod output;

use clap::Parser;
use cli::{Cli, Command, ConfigCommand, RepositoryCommand, StorageCommand};
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
                StorageCommand::AdoptLegacy { confirm, json } => {
                    rit_storage::adopt_legacy(&data, confirm)
                        .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
                    output::value(
                        &serde_json::json!({"schema_version":1,"adopted":true}),
                        json,
                    )
                    .map_err(|e| (RUNTIME_FAILURE, e))
                }
            }
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
            let git =
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let ins = git
                .inspect(&path)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"inspection":ins}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Repository {
            command: RepositoryCommand::Branches { path, json },
        } => {
            let git =
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let refs = git
                .references(&path)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?
                .into_iter()
                .filter(|r| {
                    r.name.starts_with("refs/heads/") || r.name.starts_with("refs/remotes/")
                })
                .collect::<Vec<_>>();
            output::value(
                &serde_json::json!({"schema_version":1,"references":refs}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Repository {
            command: RepositoryCommand::Refs { path, json },
        } => {
            let git =
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let refs = git
                .references(&path)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"references":refs}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Repository {
            command: RepositoryCommand::Log { path, limit, json },
        } => {
            let git =
                rit_git::Git::discover(git_arg).map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            let commits = git
                .history(&path, limit)
                .map_err(|e| (RUNTIME_FAILURE, e.to_string()))?;
            output::value(
                &serde_json::json!({"schema_version":1,"commits":commits}),
                json,
            )
            .map_err(|e| (RUNTIME_FAILURE, e))
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
