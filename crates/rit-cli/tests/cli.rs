use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let home = TempDir::new().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rit"));
    cmd.args(args)
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("xdg"))
        .env("APPDATA", home.path().join("appdata"));
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}
fn text(data: &[u8]) -> String {
    String::from_utf8(data.to_vec()).unwrap()
}
#[test]
fn help_and_version() {
    for args in [&["--help"][..], &["help"][..]] {
        let out = run(args, &[]);
        assert!(out.status.success());
        let s = text(&out.stdout);
        assert!(s.contains("doctor") && s.contains("config") && s.contains("Examples:"));
        assert!(!s.contains("checkpoint"));
    }
    let out = run(&["--version"], &[]);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stdout).trim(),
        format!("rit {}", env!("CARGO_PKG_VERSION"))
    );
}
#[test]
fn doctor_and_json() {
    let out = run(&["doctor"], &[]);
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("Configuration: valid"));
    let out = run(&["doctor", "--json"], &[]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["configuration"]["status"], "valid");
    assert_eq!(report["configuration"]["file"], "missing_using_defaults");
    assert_eq!(report["rit_version"], env!("CARGO_PKG_VERSION"));
}
#[test]
fn config_layers_and_logging_are_separate_from_json() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("rit.toml");
    fs::write(
        &path,
        "schema_version = 1\nlog_level = 'warn'\nlog_format = 'text'\n",
    )
    .unwrap();
    let path = path.to_str().unwrap();
    let out = run(
        &["--config", path, "--log-level", "debug", "doctor", "--json"],
        &[("RIT_LOG_LEVEL", "error"), ("RIT_LOG_FORMAT", "json")],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["configuration"]["file"], "loaded");
    assert_eq!(report["configuration"]["log_level"], "debug");
    assert_eq!(report["configuration"]["log_format"], "json");
    let log: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(log["level"], "DEBUG");
    let out = run(&["--config", path, "config", "validate"], &[]);
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("Configuration valid"));
}
#[test]
fn invalid_configuration_and_missing_explicit_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("rit.toml");
    let p = path.to_str().unwrap();
    for input in [
        "schema_version = 9",
        "log_level = 'wat'",
        "log_format = 'xml'",
        "unknown = 1",
        "broken = [",
    ] {
        fs::write(&path, input).unwrap();
        let out = run(&["--config", p, "config", "validate"], &[]);
        assert_eq!(out.status.code(), Some(3), "{input}");
        assert!(out.stdout.is_empty());
        assert!(text(&out.stderr).contains("configuration failed"));
    }
    // A malformed value can contain a secret; do not echo parser source contents.
    fs::write(&path, "log_level = 'super-secret-token'\n").unwrap();
    let out = run(&["--config", p, "config", "validate"], &[]);
    assert_eq!(out.status.code(), Some(3));
    assert!(!text(&out.stderr).contains("super-secret-token"));
    assert!(text(&out.stderr).contains("invalid setting value"));
    fs::remove_file(&path).unwrap();
    let out = run(&["--config", p, "doctor", "--json"], &[]);
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("cannot read"));
    let out = run(
        &[
            "--config",
            dir.path().to_str().unwrap(),
            "config",
            "validate",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(3));
}
#[test]
fn invalid_overrides_usage_and_redaction() {
    let secret = "secret-not-a-level";
    let out = run(&["doctor"], &[("RIT_LOG_LEVEL", secret)]);
    assert_eq!(out.status.code(), Some(3));
    assert!(!text(&out.stderr).contains(secret));
    let out = run(&["--log-format", "xml", "config", "validate"], &[]);
    assert_eq!(out.status.code(), Some(3));
    assert!(text(&out.stderr).contains("text or json"));
    let out = run(&["workspace", "create"], &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
