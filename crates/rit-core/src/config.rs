//! Deterministic configuration resolution, independent of process globals.
//!
//! Callers supply the file path and overrides. Missing implicit files are allowed;
//! missing explicit files are errors. This library never writes configuration files.

use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

/// The only supported configuration schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// A validated logging level.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Error events only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Informational events and above.
    #[default]
    Info,
    /// Debug events and above.
    Debug,
    /// Trace events and above.
    Trace,
}
impl FromStr for LogLevel {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(ConfigError::InvalidValue {
                field: "log level",
                expected: "error, warn, info, debug, or trace",
            }),
        }
    }
}

/// The diagnostic log encoding.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text on stderr.
    #[default]
    Text,
    /// JSON lines on stderr.
    Json,
}
impl FromStr for LogFormat {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(ConfigError::InvalidValue {
                field: "log format",
                expected: "text or json",
            }),
        }
    }
}

/// Supported Phase 1 settings. Unknown TOML fields are rejected.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Configuration schema, currently 1.
    pub schema_version: u32,
    /// Maximum verbosity of diagnostic logging.
    pub log_level: LogLevel,
    /// Diagnostic log format.
    pub log_format: LogFormat,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            log_level: LogLevel::Info,
            log_format: LogFormat::Text,
        }
    }
}

/// Optional values from one configuration layer.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    /// Override for logging level.
    pub log_level: Option<String>,
    /// Override for logging encoding.
    pub log_format: Option<String>,
}

/// A selected file and whether absence is an error.
#[derive(Clone, Debug)]
pub struct ConfigFile {
    /// Path to the file.
    pub path: PathBuf,
    /// An explicit user selection is required to exist.
    pub required: bool,
}

/// Provenance of the file layer; values themselves are not disclosed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// No path could be discovered.
    NotSelected,
    /// An implicit path did not exist.
    Missing,
    /// A file was read and parsed.
    Loaded,
}

/// Validated configuration and its file-layer status.
#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    /// Effective settings after precedence resolution.
    pub config: Config,
    /// Status of the selected file.
    pub file_state: FileState,
}

impl Config {
    /// Parse a TOML file's content and validate its schema.
    pub fn parse(input: &str, path: &Path) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(input).map_err(|source: toml::de::Error| {
            let message = source.message();
            let reason = if message.contains("unknown field") {
                "unknown field"
            } else if message.contains("unknown variant") || message.contains("invalid value") {
                "invalid setting value"
            } else {
                "invalid TOML syntax or field type"
            };
            ConfigError::Parse {
                path: path.to_owned(),
                reason,
                source: Box::new(source),
            }
        })?;
        if config.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::Schema {
                found: config.schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(config)
    }
    fn apply(&mut self, overrides: &Overrides) -> Result<(), ConfigError> {
        if let Some(v) = &overrides.log_level {
            self.log_level = v.parse()?;
        }
        if let Some(v) = &overrides.log_format {
            self.log_format = v.parse()?;
        }
        Ok(())
    }
}

/// Resolve defaults, file, environment overrides, then CLI overrides.
///
/// Both override layers are validated even if a higher layer would replace them.
/// A broken or unknown file is never silently ignored.
pub fn resolve(
    file: Option<&ConfigFile>,
    environment: &Overrides,
    cli: &Overrides,
) -> Result<ResolvedConfig, ConfigError> {
    let (mut config, file_state) = match file {
        None => (Config::default(), FileState::NotSelected),
        Some(file) => match fs::read_to_string(&file.path) {
            Ok(contents) => (Config::parse(&contents, &file.path)?, FileState::Loaded),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound && !file.required => {
                (Config::default(), FileState::Missing)
            }
            Err(source) => {
                return Err(ConfigError::Read {
                    path: file.path.clone(),
                    source,
                });
            }
        },
    };
    config.apply(environment)?;
    config.apply(cli)?;
    Ok(ResolvedConfig { config, file_state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn defaults_and_round_trip() {
        let config = Config::default();
        let text = toml::to_string(&config).unwrap();
        assert_eq!(
            Config::parse(&text, Path::new("test.toml")).unwrap(),
            config
        );
        assert_eq!(config.schema_version, 1);
    }
    #[test]
    fn rejects_bad_schema_unknown_fields_and_invalid_values() {
        for input in [
            "schema_version = 2",
            "unknown = true",
            "log_level = 'invalid'",
            "log_format = 'xml'",
            "=bad",
        ] {
            assert!(
                Config::parse(input, Path::new("test.toml")).is_err(),
                "{input}"
            );
        }
        assert!(matches!(
            Config::parse("schema_version = 99", Path::new("test")),
            Err(ConfigError::Schema { found: 99, .. })
        ));
    }
    #[test]
    fn precedence_and_invalid_lower_layer() {
        let env = Overrides {
            log_level: Some("debug".into()),
            log_format: Some("json".into()),
        };
        let cli = Overrides {
            log_level: Some("warn".into()),
            log_format: None,
        };
        let r = resolve(None, &env, &cli).unwrap();
        assert_eq!(r.config.log_level, LogLevel::Warn);
        assert_eq!(r.config.log_format, LogFormat::Json);
        let bad = Overrides {
            log_level: Some("secret-invalid-value".into()),
            ..Overrides::default()
        };
        let err = resolve(None, &bad, &cli).unwrap_err();
        assert!(!err.to_string().contains("secret-invalid-value"));
    }
    #[test]
    fn missing_file_modes_and_error_source() {
        use std::error::Error;
        let path = std::env::temp_dir().join(format!(
            "rit-nonexistent-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let defaults = Overrides::default();
        let optional = ConfigFile {
            path: path.clone(),
            required: false,
        };
        assert_eq!(
            resolve(Some(&optional), &defaults, &defaults)
                .unwrap()
                .file_state,
            FileState::Missing
        );
        let required = ConfigFile {
            path,
            required: true,
        };
        let err = resolve(Some(&required), &defaults, &defaults).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
        assert!(err.source().is_some());
    }
}
