use rit_core::config::{Config, FileState, LogFormat, LogLevel, ResolvedConfig};
use serde::Serialize;
use std::{
    io::{self, Write},
    path::Path,
};
use tracing::Level;

pub fn init_logging(config: &Config) -> Result<(), String> {
    let level = match config.log_level {
        LogLevel::Error => Level::ERROR,
        LogLevel::Warn => Level::WARN,
        LogLevel::Info => Level::INFO,
        LogLevel::Debug => Level::DEBUG,
        LogLevel::Trace => Level::TRACE,
    };
    let builder = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_max_level(level)
        .with_ansi(false);
    match config.log_format {
        LogFormat::Text => builder.try_init(),
        LogFormat::Json => builder.json().try_init(),
    }
    .map_err(|e| format!("cannot initialise logging: {e}"))
}

#[derive(Serialize)]
struct Doctor<'a> {
    schema_version: u32,
    rit_version: &'a str,
    os: &'a str,
    architecture: &'a str,
    configuration: ConfigStatus,
    git: GitStatus,
    storage: StorageStatus,
    overall_status: &'static str,
    security_gate: &'static str,
}
#[derive(Serialize)]
struct ConfigStatus {
    status: &'static str,
    file: &'static str,
    schema_version: u32,
    log_level: LogLevel,
    log_format: LogFormat,
}

#[derive(Serialize)]
struct GitStatus {
    status: &'static str,
    executable: Option<String>,
    version: Option<String>,
    error: Option<String>,
}
#[derive(Serialize)]
struct StorageStatus {
    status: &'static str,
    path: Option<String>,
    schema_version: Option<i64>,
    error: Option<String>,
}

pub fn doctor(
    resolved: &ResolvedConfig,
    json: bool,
    git_executable: Option<&Path>,
    data_dir: Option<&Path>,
) -> Result<(), String> {
    let git = match rit_git::Git::discover(git_executable.map(Path::as_os_str)) {
        Ok(g) => GitStatus {
            status: "ok",
            executable: g.executable().to_str().map(str::to_owned),
            version: Some(g.version().into()),
            error: None,
        },
        Err(e) => GitStatus {
            status: "error",
            executable: None,
            version: None,
            error: Some(e.to_string()),
        },
    };
    let storage = match rit_storage::data_dir(data_dir) {
        Ok(path) => {
            let display = path.to_str().map(str::to_owned);
            match rit_storage::diagnose(&path) {
                Ok(None) => StorageStatus {
                    status: "not_initialized",
                    path: display,
                    schema_version: None,
                    error: None,
                },
                Ok(Some(1)) => StorageStatus {
                    status: "schema_readable_security_unverified",
                    path: display,
                    schema_version: Some(1),
                    error: None,
                },
                Ok(Some(v)) => StorageStatus {
                    status: "migration_required",
                    path: display,
                    schema_version: Some(v),
                    error: None,
                },
                Err(e) => StorageStatus {
                    status: "error",
                    path: display,
                    schema_version: None,
                    error: Some(e.to_string()),
                },
            }
        }
        Err(e) => StorageStatus {
            status: "error",
            path: None,
            schema_version: None,
            error: Some(e.to_string()),
        },
    };
    let failed = git.status == "error"
        || storage.status == "error"
        || storage.status == "migration_required";
    let report = Doctor {
        schema_version: 2,
        rit_version: rit_core::version::VERSION,
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        configuration: ConfigStatus {
            status: "valid",
            file: match resolved.file_state {
                FileState::Loaded => "loaded",
                FileState::Missing => "missing_using_defaults",
                FileState::NotSelected => "not_selected_using_defaults",
            },
            schema_version: resolved.config.schema_version,
            log_level: resolved.config.log_level,
            log_format: resolved.config.log_format,
        },
        git,
        storage,
        overall_status: if failed {
            "unavailable"
        } else {
            "security_blocked"
        },
        security_gate: "blocked_phase_2_1",
    };
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        serde_json::to_writer(&mut out, &report)
            .map_err(|e| format!("cannot write doctor JSON: {e}"))?;
        writeln!(out).map_err(|e| format!("cannot write doctor output: {e}"))?;
    } else {
        writeln!(
            out,
            "RIT {}
OS: {}
Architecture: {}
Configuration: {} ({})
Logging: {:?} / {:?}\nGit: {} (executable {:?}, version {:?}, error {:?})\nStorage: {} (path {:?}, schema {:?}, error {:?})\nOverall: {} (security gate: {})",
            report.rit_version,
            report.os,
            report.architecture,
            report.configuration.status,
            report.configuration.file,
            report.configuration.log_level,
            report.configuration.log_format,
            report.git.status,
            report.git.executable,
            report.git.version,
            report.git.error,
            report.storage.status,
            report.storage.path,
            report.storage.schema_version,
            report.storage.error,
            report.overall_status,
            report.security_gate
        )
        .map_err(|e| format!("cannot write doctor output: {e}"))?;
    }
    if failed {
        return Err("one or more doctor checks failed".into());
    }
    Ok(())
}

/// Print successful validation without panicking on an output error.
pub fn config_valid(schema_version: u32) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "Configuration valid (schema {}).", schema_version)
        .map_err(|e| format!("cannot write configuration output: {e}"))
}

/// Write a structured result without mixing diagnostics into stdout.
pub fn value(value: &serde_json::Value, json: bool) -> Result<(), String> {
    let out = io::stdout();
    let mut out = out.lock();
    if json {
        serde_json::to_writer(&mut out, value)
    } else {
        serde_json::to_writer_pretty(&mut out, value)
    }
    .map_err(|e| format!("cannot write output: {e}"))?;
    writeln!(out).map_err(|e| format!("cannot write output: {e}"))
}
