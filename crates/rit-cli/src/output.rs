use rit_core::config::{Config, FileState, LogFormat, LogLevel, ResolvedConfig};
use serde::Serialize;
use std::io::{self, Write};
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
}
#[derive(Serialize)]
struct ConfigStatus {
    status: &'static str,
    file: &'static str,
    schema_version: u32,
    log_level: LogLevel,
    log_format: LogFormat,
}

pub fn doctor(resolved: &ResolvedConfig, json: bool) -> Result<(), String> {
    let report = Doctor {
        schema_version: 1,
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
Logging: {:?} / {:?}",
            report.rit_version,
            report.os,
            report.architecture,
            report.configuration.status,
            report.configuration.file,
            report.configuration.log_level,
            report.configuration.log_format
        )
        .map_err(|e| format!("cannot write doctor output: {e}"))?;
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
