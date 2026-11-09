//! Typed errors for configuration loading and validation.

use std::path::PathBuf;

/// A configuration operation failed.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Reading a selected configuration file failed.
    #[error("cannot read configuration file {path:?}: {source}")]
    Read {
        /// The selected file.
        path: PathBuf,
        /// The underlying operating system error.
        #[source]
        source: std::io::Error,
    },
    /// Configuration could not be parsed. File contents are not included in the message.
    #[error("invalid configuration in {path:?}: {reason}; check the configuration documentation")]
    Parse {
        /// The selected file.
        path: PathBuf,
        /// Safe, non-content-bearing error category.
        reason: &'static str,
        /// The underlying TOML parser error.
        #[source]
        source: Box<toml::de::Error>,
    },
    /// The configuration uses an unsupported schema.
    #[error("unsupported configuration schema version {found}; expected {expected}")]
    Schema {
        /// Detected schema version.
        found: u32,
        /// Supported schema version.
        expected: u32,
    },
    /// An override has an invalid value.
    #[error("invalid {field}; expected {expected}")]
    InvalidValue {
        /// Field to correct.
        field: &'static str,
        /// Permitted values.
        expected: &'static str,
    },
    /// An environment value could not be interpreted as Unicode.
    #[error("{name} must contain valid Unicode")]
    Environment {
        /// Name of the invalid environment variable.
        name: &'static str,
    },
}
