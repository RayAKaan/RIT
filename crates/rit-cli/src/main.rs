//! CLI startup; all reusable configuration behavior lives in rit-core.
#![forbid(unsafe_code)]
mod cli;
mod output;

use clap::Parser;
use cli::{Cli, Command, ConfigCommand};
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
    match cli.command {
        Command::Doctor { json } => {
            output::doctor(&resolved, json).map_err(|e| (RUNTIME_FAILURE, e))
        }
        Command::Config {
            command: ConfigCommand::Validate,
        } => output::config_valid(resolved.config.schema_version).map_err(|e| (RUNTIME_FAILURE, e)),
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
