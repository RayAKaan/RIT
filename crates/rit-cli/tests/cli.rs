use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let home = TempDir::new().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rit"));
    cmd.args(args)
        .env_remove("RIT_DATA_DIR")
        .env_remove("RIT_GIT_EXECUTABLE")
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("xdg"))
        .env("APPDATA", home.path().join("appdata"))
        .env("LOCALAPPDATA", home.path().join("localappdata"))
        .env("XDG_DATA_HOME", home.path().join("data"));
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
        assert!(s.contains("checkpoint"));
        assert!(s.contains("currently unavailable"));
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
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["git"]["status"], "ok");
    assert_eq!(report["storage"]["status"], "not_initialized");
    assert_eq!(report["security_gate"], "blocked_phase_2_1");
    assert_eq!(report["overall_status"], "security_blocked");
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

#[test]
fn security_scaffolding_fails_closed_without_touching_storage() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    for args in [
        &["identity", "bootstrap"][..],
        &["workspace", "create", "repo"],
        &["agent", "run", "workspace"],
        &["checkpoint", "restore", "checkpoint"],
        &["submission", "publish", "submission"],
        &["task", "review", "submission"],
        &["whoami"],
        &["permission", "roles"],
        &["audit", "verify"],
        &["identity", "credential", "issue", "test"],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .arg("--data-dir")
            .arg(&data)
            .args(args)
            .env("XDG_CONFIG_HOME", d.path().join("config"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(4));
        assert!(String::from_utf8_lossy(&out.stderr).contains("secure registry storage"));
        assert!(!data.exists());
    }
}

#[test]
fn read_only_git_refs_and_log_do_not_open_registry() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let exec = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success());
    };
    exec(&["init", "-q"]);
    exec(&["commit", "-q", "--allow-empty", "-m", "first"]);
    exec(&["tag", "v1"]);
    let data = d.path().join("not-created");
    for cmd in ["branches", "refs", "log"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args([
                "--data-dir",
                data.to_str().unwrap(),
                "repo",
                cmd,
                repo.to_str().unwrap(),
                "--json",
            ])
            .env("XDG_CONFIG_HOME", d.path().join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema_version"], 1);
        let values = if cmd == "log" {
            &value["commits"]
        } else {
            &value["references"]
        };
        assert!(!values.as_array().unwrap().is_empty());
        if cmd == "branches" {
            assert!(
                values
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| !r["name"].as_str().unwrap().starts_with("refs/tags/"))
            );
        }
        assert!(!data.exists());
    }
}

#[test]
fn legacy_adoption_cli_fails_closed_without_creating_storage() {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("missing");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(&data)
        .args(["storage", "adopt-legacy", "--confirm"])
        .env("XDG_CONFIG_HOME", d.path().join("config"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&out.stderr).contains("legacy adoption requires"));
    assert!(!data.exists());
}

#[test]
fn repository_read_commands_use_real_git_without_registry() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let data = d.path().join("no-registry");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success());
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("source.txt"), "base\n").unwrap();
    git(&["add", "--", "source.txt"]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["tag", "v1"]);
    std::fs::write(repo.join("source.txt"), "changed\n").unwrap();
    for cmd in [
        "inspect",
        "status",
        "branches",
        "tags",
        "log",
        "diff",
        "refs",
        "worktrees",
        "doctor",
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .arg("--data-dir")
            .arg(&data)
            .args(["repo", cmd])
            .arg(&repo)
            .arg("--json")
            .env("XDG_CONFIG_HOME", d.path().join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["schema_version"], 1);
        if cmd == "diff" {
            assert_eq!(v["diff"]["unstaged"][0]["path"], "source.txt");
        }
        if cmd == "tags" {
            assert_eq!(v["references"][0]["name"], "refs/tags/v1");
        }
        if cmd == "worktrees" {
            assert_eq!(v["worktrees"].as_array().unwrap().len(), 1);
        }
        if cmd == "doctor" {
            assert_eq!(v["security_gate"], "blocked_phase_2_1");
        }
        assert!(!data.exists());
    }
}
