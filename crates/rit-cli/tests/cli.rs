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
    // The legacy positional workspace scaffold is no longer a valid invocation;
    // new creation requires explicit --repo, --path and --name and is separately tested.
    let legacy = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(&data)
        .args(["workspace", "create", "repo"])
        .output()
        .unwrap();
    assert_eq!(legacy.status.code(), Some(2));
    assert!(!data.exists());
    for args in [
        &["identity", "bootstrap"][..],
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

#[test]
fn real_committed_dependency_graph_and_impact_are_revision_bound() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let data = d.path().join("no-registry");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
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
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::create_dir(repo.join("pkg")).unwrap();
    std::fs::write(repo.join("pkg/base.py"), "def f(): pass\n").unwrap();
    std::fs::write(repo.join("pkg/mid.py"), "from .base import f\n").unwrap();
    std::fs::write(
        repo.join("pkg/top.py"),
        "from pkg.mid import f\nimport external_module\n",
    )
    .unwrap();
    std::fs::write(repo.join("app.ts"), "import {x} from './util';\n").unwrap();
    std::fs::write(repo.join("util.ts"), "export const x=1;\n").unwrap();
    git(&["add", "--all"]);
    git(&["commit", "-q", "-m", "source"]);
    let head = git(&["rev-parse", "HEAD"]);
    // Uncommitted edits are not misrepresented as analyzed HEAD content.
    std::fs::write(repo.join("pkg/top.py"), "# dirty state ignored\n").unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .arg("--data-dir")
            .arg(&data)
            .args(args)
            .env("XDG_CONFIG_HOME", d.path().join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let p = repo.to_str().unwrap();
    let graph = run(&["dependencies", "inspect", "--repo", p, "--json"]);
    assert_eq!(graph["graph"]["commit"]["hex"], head);
    assert_eq!(graph["graph"]["complete"], false);
    let edges = graph["graph"]["edges"].as_array().unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e["dependent"] == "pkg/mid.py" && e["dependency"] == "pkg/base.py")
    );
    assert!(
        edges
            .iter()
            .any(|e| e["dependent"] == "app.ts" && e["dependency"] == "util.ts")
    );
    assert!(
        graph["graph"]["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["specifier"] == "external_module")
    );
    let impact = run(&["impact", "--repo", p, "--files", "pkg/base.py", "--json"]);
    let result = impact["reports"][0]["known_dependents"].as_array().unwrap();
    assert!(result.iter().any(|s| s == "pkg/mid.py"));
    assert!(result.iter().any(|s| s == "pkg/top.py"));
    assert!(!data.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn commit_only_checkpoint_is_real_and_preserves_all_uncommitted_work() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let data = d.path().join("none");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("tracked"), "base\n").unwrap();
    git(&["add", "tracked"]);
    git(&["commit", "-q", "-m", "base"]);
    let head = git(&["rev-parse", "HEAD"]);
    std::fs::write(repo.join("tracked"), "staged\n").unwrap();
    git(&["add", "tracked"]);
    std::fs::write(repo.join("tracked"), "unstaged\n").unwrap();
    std::fs::write(repo.join("untracked"), "user data").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let hook = repo.join(".git/hooks/reference-transaction");
    std::fs::write(&hook, "#!/bin/sh\ntouch executed-hook\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .arg("--data-dir")
            .arg(&data)
            .args(args)
            .env("XDG_CONFIG_HOME", d.path().join("config"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let path = repo.to_str().unwrap();
    let created = run(&[
        "checkpoint",
        "create",
        "--repo",
        path,
        "--commit-only",
        "--json",
    ]);
    let item = &created["checkpoint"];
    assert_eq!(item["commit"]["hex"], head);
    assert!(
        item["capture_scope"]
            .as_str()
            .unwrap()
            .contains("index, working tree, untracked")
    );
    assert!(!repo.join("executed-hook").exists());
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("untracked")).unwrap(),
        "user data"
    );
    let id = item["id"].as_str().unwrap();
    let list = run(&["checkpoint", "list", "--repo", path, "--json"]);
    assert_eq!(list["checkpoints"].as_array().unwrap().len(), 1);
    let verify = run(&["checkpoint", "verify", id, "--repo", path, "--json"]);
    assert_eq!(verify["git_fsck"], "passed");
    assert_eq!(verify["uncommitted_work_captured"], false);
    git(&["gc", "--quiet"]);
    assert_eq!(
        run(&["checkpoint", "list", "--repo", path, "--json"])["checkpoints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert!(!data.exists());
}

#[test]
fn activity_polling_observes_five_real_git_worktrees_and_overlap() {
    use std::io::{BufRead, BufReader};
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("base");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("shared.py"), "base\n").unwrap();
    git(&["add", "shared.py"]);
    git(&["commit", "-q", "-m", "base"]);
    let mut trees = Vec::new();
    for i in 0..5 {
        let path = d.path().join(format!("wt{i}"));
        git(&[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("test{i}"),
            path.to_str().unwrap(),
            "HEAD",
        ]);
        trees.push(path);
    }
    std::fs::write(trees[0].join("shared.py"), "one\n").unwrap();
    std::fs::write(trees[1].join("shared.py"), "two\n").unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(d.path().join("none"))
        .args([
            "activity",
            "watch",
            "--repo",
            repo.to_str().unwrap(),
            "--interval-ms",
            "250",
            "--count",
            "2",
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let first: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(first["activity"]["worktrees"].as_array().unwrap().len(), 6);
    assert_eq!(first["activity"]["overlaps"][0]["path"], "shared.py");
    assert_eq!(
        first["activity"]["overlaps"][0]["worktrees"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(first["activity"]["complete_event_history"], false);
    std::fs::write(trees[2].join("shared.py"), "three\n").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let second: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        second["activity"]["overlaps"][0]["worktrees"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(child.wait().unwrap().success());
    assert!(!d.path().join("none").exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn creates_and_reconciles_five_actual_rit_workspaces() {
    use std::collections::HashSet;
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("shared.py"), "VALUE = 1\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let hook = repo.join(".git/hooks/reference-transaction");
    std::fs::write(&hook, "#!/bin/sh\ntouch executed-hook\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let data = d.path().join("no-registry");
    let cli = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .arg("--data-dir")
            .arg(&data)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let rp = repo.to_str().unwrap();
    let mut ids = HashSet::new();
    let mut branches = HashSet::new();
    let mut commits = HashSet::new();
    let mut worktrees = Vec::new();
    for n in 0..5 {
        let w = d.path().join(format!("work{n}"));
        let p = w.to_str().unwrap();
        let name = format!("workspace {n}");
        let created = cli(&[
            "workspace",
            "create",
            "--repo",
            rp,
            "--path",
            p,
            "--name",
            &name,
            "--base",
            "HEAD",
            "--json",
        ]);
        let m = &created["workspace"];
        assert_eq!(m["state"], "ready");
        assert_eq!(m["base"]["hex"], head);
        assert!(ids.insert(m["id"].as_str().unwrap().to_owned()));
        assert!(branches.insert(m["branch"].as_str().unwrap().to_owned()));
        assert_eq!(
            std::fs::read_to_string(w.join("shared.py")).unwrap(),
            "VALUE = 1\n"
        );
        std::fs::write(w.join(format!("file{n}.py")), format!("VALUE={n}\n")).unwrap();
        std::fs::write(w.join("shared.py"), format!("VALUE={n}\n")).unwrap();
        git(&w, &["-c", "core.hooksPath=/dev/null", "add", "."]);
        git(
            &w,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                &format!("change {n}"),
            ],
        );
        commits.insert(git(&w, &["rev-parse", "HEAD"]));
        let info = cli(&[
            "workspace",
            "inspect",
            m["id"].as_str().unwrap(),
            "--repo",
            rp,
            "--json",
        ]);
        assert_eq!(info["workspace"]["observation"], "present");
        worktrees.push((w, m["id"].as_str().unwrap().to_owned()));
    }
    assert_eq!(ids.len(), 5);
    assert_eq!(branches.len(), 5);
    assert_eq!(commits.len(), 5);
    assert!(!repo.join("executed-hook").exists());
    let listing = cli(&["workspace", "list", "--repo", rp, "--json"]);
    assert_eq!(
        listing["reconciliation"]["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let id = &worktrees[0].1;
    cli(&[
        "workspace",
        "lock",
        id,
        "--repo",
        rp,
        "--reason",
        "fixture",
        "--json",
    ]);
    let locked = cli(&["workspace", "status", id, "--repo", rp, "--json"]);
    assert_eq!(locked["workspace"]["worktree"]["locked"], true);
    cli(&["workspace", "unlock", id, "--repo", rp, "--json"]);
    std::fs::write(worktrees[0].0.join("untracked"), "preserve").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args(["workspace", "remove", id, "--repo", rp])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(
        std::fs::read_to_string(worktrees[0].0.join("untracked")).unwrap(),
        "preserve"
    );
    // Simulate a moved worktree and an interrupted manifest after the workspaces
    // were created by RIT; reconciliation must report, never delete, either.
    let moved = d.path().join("moved-worktree");
    git(
        &repo,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "move",
            worktrees[1].0.to_str().unwrap(),
            moved.to_str().unwrap(),
        ],
    );
    let interrupted_id = &worktrees[2].1;
    let current = cli(&[
        "workspace",
        "inspect",
        interrupted_id,
        "--repo",
        rp,
        "--json",
    ]);
    let mut manifest = current["workspace"]["manifest"].clone();
    manifest["state"] = "provisioning".into();
    let fixture = d.path().join("interrupted-manifest.json");
    std::fs::write(&fixture, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let oid = git(
        &repo,
        &[
            "hash-object",
            "-w",
            "--no-filters",
            fixture.to_str().unwrap(),
        ],
    );
    let manifest_ref = format!("refs/rit/workspaces/{interrupted_id}/manifest");
    git(
        &repo,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            &manifest_ref,
            &oid,
        ],
    );
    git(
        &repo,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "refs/heads/rit/workspaces/orphan",
            &head,
        ],
    );
    let final_state = cli(&["workspace", "reconcile", "--repo", rp, "--json"]);
    let ws = final_state["reconciliation"]["workspaces"]
        .as_array()
        .unwrap();
    assert_eq!(
        ws.iter()
            .find(|v| v["manifest"]["id"] == worktrees[1].1)
            .unwrap()["observation"],
        "moved"
    );
    assert_eq!(
        ws.iter()
            .find(|v| v["manifest"]["id"] == *interrupted_id)
            .unwrap()["observation"],
        "interrupted"
    );
    assert_eq!(
        final_state["reconciliation"]["orphan_branches"][0],
        "refs/heads/rit/workspaces/orphan"
    );
    assert!(moved.exists());
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert!(!data.exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn filtered_workspace_is_real_but_explicitly_not_checked_out() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join(".gitattributes"), "*.txt filter=bad\n").unwrap();
    std::fs::write(repo.join("example.txt"), "hello\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let marker = d.path().join("filter-executed");
    let filter = format!("sh -c 'touch {}; cat'", marker.display());
    git(&["config", "filter.bad.smudge", &filter]);
    git(&["config", "filter.bad.clean", &filter]);
    let target = d.path().join("work");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args([
            "workspace",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--path",
            target.to_str().unwrap(),
            "--name",
            "filtered",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["workspace"]["state"], "no_checkout");
    assert!(!marker.exists());
    assert!(target.join(".git").exists());
    assert!(!target.join("example.txt").exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_watcher_observes_concurrent_changes_in_rit_created_workspaces() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("shared.py"), "VALUE=0\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let mut ids = std::collections::HashSet::new();
    let mut paths = Vec::new();
    for i in 0..5 {
        let p = d.path().join(format!("work{i}"));
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args([
                "workspace",
                "create",
                "--repo",
                rp,
                "--path",
                p.to_str().unwrap(),
                "--name",
                "watch",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["workspace"]["state"], "ready");
        ids.insert(v["workspace"]["id"].as_str().unwrap().to_string());
        paths.push(p);
    }
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args([
            "activity",
            "watch",
            "--repo",
            rp,
            "--interval-ms",
            "1000",
            "--duration-ms",
            "3500",
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let start: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(start["type"], "watch_started");
    let threads = paths
        .into_iter()
        .enumerate()
        .map(|(i, path)| {
            std::thread::spawn(move || {
                std::fs::write(path.join("shared.py"), format!("VALUE={i}\n")).unwrap()
            })
        })
        .collect::<Vec<_>>();
    for t in threads {
        t.join().unwrap();
    }
    let mut seen = std::collections::HashSet::new();
    let mut reconciled = false;
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["type"] == "notifications" {
            for e in v["batch"]["events"].as_array().unwrap() {
                if e["path"] == "shared.py" {
                    if let Some(id) = e["workspace_id"].as_str() {
                        seen.insert(id.to_string());
                    }
                    assert!(e["actor_id"].is_null());
                }
            }
        }
        if v["type"] == "git_reconciliation" {
            reconciled = true;
        }
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(seen, ids);
    assert!(reconciled);
}

#[test]
fn analyzes_selected_commits_and_go_module_packages_with_evidence() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(r)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::create_dir(r.join("pkg")).unwrap();
    std::fs::write(
        r.join("go.mod"),
        "module example.invalid/project\n\ngo 1.23\n",
    )
    .unwrap();
    std::fs::write(
        r.join("main.go"),
        "package main\nimport (\"example.invalid/project/pkg\"; \"fmt\")\n",
    )
    .unwrap();
    std::fs::write(r.join("pkg/a.go"), "package pkg\n").unwrap();
    std::fs::write(r.join("x.py"), "import y\n").unwrap();
    std::fs::write(r.join("y.py"), "import x\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "first"]);
    let old = git(&["rev-parse", "HEAD"]);
    git(&["branch", "old-version"]);
    std::fs::write(r.join("main.go"), "package main\nimport \"fmt\"\n").unwrap();
    git(&["add", "main.go"]);
    git(&["commit", "-qm", "second"]);
    let head = git(&["rev-parse", "HEAD"]);
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let path = r.to_str().unwrap();
    let old_graph = run(&[
        "dependencies",
        "inspect",
        "--repo",
        path,
        "--commit",
        &old,
        "--json",
    ]);
    assert_eq!(old_graph["graph"]["commit"]["hex"], old);
    let edges = old_graph["graph"]["edges"].as_array().unwrap();
    let edge = edges
        .iter()
        .find(|e| e["dependent"] == "main.go" && e["dependency"] == "pkg/a.go")
        .unwrap();
    assert_eq!(edge["resolution_method"], "go_mod_local_package_files");
    assert_eq!(edge["snapshot_commit"], old);
    assert_eq!(edge["source_sha256"].as_str().unwrap().len(), 64);
    assert!(
        old_graph["graph"]["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["specifier"] == "fmt")
    );
    assert!(
        old_graph["graph"]["cycles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_array().unwrap().len() == 2)
    );
    let by_branch = run(&[
        "dependencies",
        "inspect",
        "--repo",
        path,
        "--branch",
        "old-version",
        "--json",
    ]);
    assert_eq!(by_branch["graph"]["commit"]["hex"], old);
    let current = run(&["dependencies", "inspect", "--repo", path, "--json"]);
    assert_eq!(current["graph"]["commit"]["hex"], head);
    assert!(
        !current["graph"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["dependent"] == "main.go")
    );
    let impact = run(&[
        "impact", "--repo", path, "--commit", &old, "--files", "pkg/a.go", "--json",
    ]);
    assert!(
        impact["reports"][0]["known_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "main.go")
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn workspace_analysis_reads_non_atomic_tracked_and_untracked_sources_without_following_symlinks() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("app.py"), "import base\n").unwrap();
    std::fs::write(repo.join("base.py"), "VALUE=1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let w = d.path().join("work");
    let rp = repo.to_str().unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let m = run(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        w.to_str().unwrap(),
        "--name",
        "analysis",
        "--json",
    ]);
    let id = m["workspace"]["id"].as_str().unwrap();
    let before = run(&[
        "dependencies",
        "inspect",
        "--repo",
        rp,
        "--workspace",
        id,
        "--json",
    ]);
    std::fs::write(w.join("app.py"), "import extra\n").unwrap();
    std::fs::write(w.join("extra.py"), "VALUE=2\n").unwrap();
    std::os::unix::fs::symlink(repo.join("base.py"), w.join("link.py")).unwrap();
    let after = run(&[
        "dependencies",
        "inspect",
        "--repo",
        rp,
        "--workspace",
        id,
        "--json",
    ]);
    assert_ne!(
        before["graph"]["snapshot_id"],
        after["graph"]["snapshot_id"]
    );
    assert_eq!(after["graph"]["non_atomic"], true);
    assert_eq!(after["graph"]["omitted_non_regular"], 1);
    assert!(
        after["graph"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["dependent"] == "app.py" && e["dependency"] == "extra.py")
    );
    let head = run(&["dependencies", "inspect", "--repo", rp, "--json"]);
    assert!(
        !head["graph"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "extra.py")
    );
    let impact = run(&["impact", "--repo", rp, "--workspace", id, "--json"]);
    assert!(impact["reports"].as_array().unwrap().iter().any(|r| {
        r["changed"] == "extra.py"
            && r["known_dependents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "app.py")
    }));
    assert_eq!(impact["non_atomic"], true);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn conflict_simulation_is_isolated_and_reports_git_stages_and_structural_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("app.py"), "def value():\n    return 0\n").unwrap();
    std::fs::write(repo.join("binary.dat"), b"base\0data\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let run = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let mut commits = Vec::new();
    let mut dirs = Vec::new();
    for n in 1..=2 {
        let w = d.path().join(format!("w{n}"));
        let created = run(&[
            "workspace",
            "create",
            "--repo",
            rp,
            "--path",
            w.to_str().unwrap(),
            "--name",
            "merge",
            "--json",
        ]);
        assert_eq!(created["workspace"]["state"], "ready");
        std::fs::write(w.join("app.py"), format!("def value():\n    return {n}\n")).unwrap();
        std::fs::write(w.join("binary.dat"), format!("binary\0{n}\n")).unwrap();
        git(&w, &["add", "."]);
        git(&w, &["commit", "-qm", &format!("side {n}")]);
        commits.push(git(&w, &["rev-parse", "HEAD"]));
        dirs.push(w);
    }
    let refs_before = git(
        &repo,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    let status_before = git(&repo, &["status", "--porcelain=v2"]);
    let result = run(&[
        "conflicts",
        "simulate",
        "--repo",
        rp,
        "--left",
        &commits[0],
        "--right",
        &commits[1],
        "--json",
    ]);
    let report = &result["conflicts"];
    assert_eq!(report["simulation"]["has_conflicts"], true);
    assert_eq!(report["simulation"]["stages"].as_array().unwrap().len(), 6);
    assert!(
        report["structural_candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["path"] == "app.py" && x["name"] == "value")
    );
    assert!(
        report["classifications"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["category"] == "binary_conflict")
    );
    assert_eq!(
        git(
            &repo,
            &["for-each-ref", "--format=%(refname) %(objectname)"]
        ),
        refs_before
    );
    assert_eq!(git(&repo, &["status", "--porcelain=v2"]), status_before);
    assert_eq!(
        std::fs::read_to_string(repo.join("app.py")).unwrap(),
        "def value():\n    return 0\n"
    );
    assert_eq!(git(&dirs[0], &["rev-parse", "HEAD"]), commits[0]);
    assert_eq!(git(&dirs[1], &["rev-parse", "HEAD"]), commits[1]);
    let marker = d.path().join("driver-executed");
    git(
        &repo,
        &[
            "config",
            "merge.evil.driver",
            &format!("sh -c 'touch {}'", marker.display()),
        ],
    );
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args([
            "conflicts",
            "simulate",
            "--repo",
            rp,
            "--left",
            &commits[0],
            "--right",
            &commits[1],
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(!marker.exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn content_checkpoint_captures_staged_raw_worktree_untracked_ignored_and_links_without_touching_index()
 {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(repo.join("tracked.txt"), "base\n").unwrap();
    std::fs::write(repo.join("script.sh"), "#!/bin/sh\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let run = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = d.path().join("work");
    let workspace = run(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        w.to_str().unwrap(),
        "--name",
        "checkpoint",
        "--json",
    ]);
    let wid = workspace["workspace"]["id"].as_str().unwrap();
    std::fs::write(w.join("tracked.txt"), "staged\n").unwrap();
    git(&w, &["add", "tracked.txt"]);
    let staged_oid = git(&w, &["rev-parse", ":tracked.txt"]);
    std::fs::write(w.join("tracked.txt"), "unstaged\n").unwrap();
    std::fs::write(w.join("untracked.txt"), "new data\n").unwrap();
    std::fs::write(w.join("ignored.txt"), "ignored data\n").unwrap();
    symlink("tracked.txt", w.join("link.txt")).unwrap();
    std::fs::set_permissions(w.join("script.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let idx = git(&w, &["rev-parse", "--git-path", "index"]);
    let original_index = std::fs::read(&idx).unwrap();
    let snapshot = run(&[
        "checkpoint",
        "create",
        "--repo",
        rp,
        "--workspace",
        wid,
        "--include-untracked",
        "--include-ignored",
        "ignored.txt",
        "--json",
    ]);
    let c = &snapshot["checkpoint"];
    let id = c["manifest"]["id"].as_str().unwrap();
    let manifest = &c["manifest"];
    assert_eq!(manifest["complete"], false);
    assert_eq!(std::fs::read(&idx).unwrap(), original_index);
    let stage = manifest["staged"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["path"] == "tracked.txt")
        .unwrap();
    assert_eq!(stage["object"]["hex"], staged_oid);
    let entries = manifest["worktree"].as_array().unwrap();
    for (name, expected, category) in [
        ("tracked.txt", "unstaged", "tracked"),
        ("untracked.txt", "new data", "untracked"),
        ("ignored.txt", "ignored data", "selected_ignored"),
        ("link.txt", "tracked.txt", "untracked"),
    ] {
        let e = entries.iter().find(|x| x["path"] == name).unwrap();
        assert_eq!(e["category"], category);
        assert_eq!(
            git(
                &repo,
                &["cat-file", "blob", e["blob"]["hex"].as_str().unwrap()]
            ),
            expected
        );
    }
    assert_eq!(
        entries.iter().find(|x| x["path"] == "link.txt").unwrap()["kind"],
        "symlink"
    );
    assert_eq!(
        entries.iter().find(|x| x["path"] == "script.sh").unwrap()["executable"],
        true
    );
    let verify = run(&["checkpoint", "verify", id, "--repo", rp, "--json"]);
    assert_eq!(verify["git_fsck"], "passed");
    assert_eq!(verify["manifest_tree_consistency"], "passed");
    assert_eq!(verify["atomic_completeness"], false);
    git(&repo, &["gc", "--quiet"]);
    let list = run(&["checkpoint", "list", "--repo", rp, "--json"]);
    assert_eq!(list["content_checkpoints"].as_array().unwrap().len(), 1);
    run(&["checkpoint", "verify", id, "--repo", rp, "--json"]);
    // Substitute a structurally valid Git commit with an index tree entry inconsistent
    // with its unchanged manifest. This is integrity detection, not authentication.
    let original = c["object"]["hex"].as_str().unwrap();
    let tree_text = git(&repo, &["ls-tree", original]);
    let changed_tree = tree_text
        .lines()
        .map(|line| {
            if line.ends_with("\tindex") {
                format!("100644 blob {}\tindex", staged_oid)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    use std::io::Write as _;
    let mut process = std::process::Command::new("git")
        .args(["mktree"])
        .current_dir(&repo)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(changed_tree.as_bytes())
        .unwrap();
    let out = process.wait_with_output().unwrap();
    assert!(out.status.success());
    let changed_tree = String::from_utf8(out.stdout).unwrap();
    let forged = git(
        &repo,
        &[
            "commit-tree",
            changed_tree.trim(),
            "-p",
            manifest["base"]["hex"].as_str().unwrap(),
            "-m",
            "forged metadata",
        ],
    );
    git(
        &repo,
        &[
            "update-ref",
            c["reference"].as_str().unwrap(),
            &forged,
            original,
        ],
    );
    let verify_bad = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args(["checkpoint", "verify", id, "--repo", rp, "--json"])
        .output()
        .unwrap();
    assert!(!verify_bad.status.success());
    git(
        &repo,
        &[
            "update-ref",
            c["reference"].as_str().unwrap(),
            original,
            &forged,
        ],
    );
    run(&["checkpoint", "verify", id, "--repo", rp, "--json"]);
    assert_eq!(std::fs::read(&idx).unwrap(), original_index);
    let restore = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args(["checkpoint", "restore", id])
        .output()
        .unwrap();
    assert!(!restore.status.success());
    assert_eq!(
        std::fs::read_to_string(w.join("tracked.txt")).unwrap(),
        "unstaged\n"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn submission_builds_real_detached_candidate_without_publishing_and_reconciles_staleness() {
    let d = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    fn git(p: &std::path::Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(p)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    }
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(repo.join("base.py"), "value = 1\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let work = d.path().join("work");
    let wp = work.to_str().unwrap();
    let cli = |a: &[&str]| {
        let o = run(a, &[]);
        assert!(o.status.success(), "{a:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        wp,
        "--name",
        "source",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    std::fs::write(
        work.join("feature.py"),
        "from base import value\nresult = value\n",
    )
    .unwrap();
    let dirty = run(
        &[
            "submission",
            "create",
            wid,
            "--repo",
            rp,
            "--target",
            "refs/heads/main",
            "--json",
        ],
        &[],
    );
    assert!(!dirty.status.success());
    assert!(text(&dirty.stderr).contains("clean workspace"));
    git(&work, &["add", "feature.py"]);
    git(&work, &["commit", "-qm", "feature"]);
    let source = git(&work, &["rev-parse", "HEAD"]);
    let target = git(&repo, &["rev-parse", "refs/heads/main"]);
    let hook = repo.join(".git/hooks/reference-transaction");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            d.path().join("executed-hook").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let receipt = cli(&[
        "submission",
        "create",
        wid,
        "--repo",
        rp,
        "--target",
        "refs/heads/main",
        "--json",
    ]);
    let sub = &receipt["receipt"]["submission"];
    let sid = sub["id"].as_str().unwrap();
    assert_eq!(sub["source_commit"]["hex"], source);
    assert_eq!(sub["target_commit"]["hex"], target);
    assert!(
        sub["changed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "feature.py")
    );
    assert!(receipt["receipt"]["dependency_analysis"].is_object());
    let candidate = d.path().join("candidate");
    let cp = candidate.to_str().unwrap();
    let output = cli(&[
        "submission",
        "candidate",
        sid,
        "--repo",
        rp,
        "--path",
        cp,
        "--json",
    ]);
    let result = &output["result"];
    assert_eq!(result["status"], "created");
    assert_eq!(result["candidate"]["observation"], "present");
    assert_eq!(result["candidate"]["target_moved"], false);
    assert_eq!(
        result["object_check"],
        "passed (Git fsck; auxiliary indexes excluded)"
    );
    assert!(result["dependency_analysis"].is_object());
    assert!(result["execution"].as_str().unwrap().starts_with("not run"));
    let cid = result["candidate"]["candidate"]["id"].as_str().unwrap();
    let commit = result["candidate"]["candidate"]["commit"]["hex"]
        .as_str()
        .unwrap();
    assert_eq!(git(&candidate, &["rev-parse", "HEAD"]), commit);
    assert_eq!(git(&candidate, &["rev-parse", "HEAD^1"]), target);
    assert_eq!(git(&candidate, &["rev-parse", "HEAD^2"]), source);
    assert_eq!(
        std::fs::read_to_string(candidate.join("feature.py")).unwrap(),
        "from base import value\nresult = value\n"
    );
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), target);
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), source);
    assert_eq!(git(&work, &["status", "--porcelain"]), "");
    let listed = cli(&["submission", "list", "--repo", rp, "--json"]);
    assert_eq!(listed["submissions"][0]["id"], sid);
    assert!(!d.path().join("executed-hook").exists());
    std::fs::remove_file(&hook).unwrap();
    git(&repo, &["commit", "--allow-empty", "-qm", "move target"]);
    let candidates = cli(&["submission", "candidates", "--repo", rp, "--json"]);
    assert_eq!(candidates["candidates"][0]["target_moved"], true);
    let stale = run(
        &[
            "submission",
            "candidate",
            sid,
            "--repo",
            rp,
            "--path",
            d.path().join("stale").to_str().unwrap(),
            "--json",
        ],
        &[],
    );
    assert!(!stale.status.success());
    assert!(text(&stale.stderr).contains("target moved"));
    assert!(!d.path().join("stale").exists());
    git(
        &repo,
        &[
            "update-ref",
            "-d",
            &format!("refs/rit/candidates/{cid}/commit"),
        ],
    );
    let observed = cli(&["submission", "candidates", "--repo", rp, "--json"]);
    assert_eq!(
        observed["candidates"][0]["observation"],
        "candidate_ref_mismatch"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn conflicting_submission_reports_git_stages_and_creates_no_candidate() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("app.py"), "def f():\n    return 0\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let work = d.path().join("source");
    let wp = work.to_str().unwrap();
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        wp,
        "--name",
        "source",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    std::fs::write(work.join("app.py"), "def f():\n    return 1\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-qm", "source"]);
    std::fs::write(repo.join("app.py"), "def f():\n    return 2\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "target"]);
    let target = git(&repo, &["rev-parse", "HEAD"]);
    let source = git(&work, &["rev-parse", "HEAD"]);
    let sub = cli(&[
        "submission",
        "create",
        wid,
        "--repo",
        rp,
        "--target",
        "refs/heads/main",
        "--json",
    ]);
    let sid = sub["receipt"]["submission"]["id"].as_str().unwrap();
    let path = d.path().join("candidate");
    let out = cli(&[
        "submission",
        "candidate",
        sid,
        "--repo",
        rp,
        "--path",
        path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out["result"]["status"], "conflicted");
    assert_eq!(
        out["result"]["conflict_report"]["simulation"]["has_conflicts"],
        true
    );
    assert!(
        !out["result"]["conflict_report"]["simulation"]["stages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!path.exists());
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), target);
    assert_eq!(git(&work, &["rev-parse", "HEAD"]), source);
    assert!(
        cli(&["submission", "candidates", "--repo", rp, "--json"])["candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn checkpoint_preserves_unmerged_index_deletion_and_refuses_large_files() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    std::fs::write(repo.join("delete.txt"), "to delete\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let work = d.path().join("work");
    let wp = work.to_str().unwrap();
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        wp,
        "--name",
        "conflicted",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    std::fs::write(work.join("file.txt"), "source\n").unwrap();
    git(&work, &["add", "file.txt"]);
    git(&work, &["commit", "-qm", "source"]);
    std::fs::write(repo.join("file.txt"), "target\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    git(&repo, &["commit", "-qm", "target"]);
    let merge = std::process::Command::new("git")
        .current_dir(&work)
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.invalid",
            "merge",
            "--no-commit",
            "main",
        ])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    std::fs::remove_file(work.join("delete.txt")).unwrap();
    let index = git(&work, &["rev-parse", "--git-path", "index"]);
    let before = std::fs::read(&index).unwrap();
    let result = cli(&[
        "checkpoint",
        "create",
        "--repo",
        rp,
        "--workspace",
        wid,
        "--json",
    ]);
    let c = &result["checkpoint"];
    let entries = c["manifest"]["staged"].as_array().unwrap();
    for stage in [1, 2, 3] {
        assert!(
            entries
                .iter()
                .any(|e| e["path"] == "file.txt" && e["stage"] == stage)
        );
    }
    assert!(
        c["manifest"]["worktree"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "delete.txt" && e["kind"] == "deleted")
    );
    let id = c["manifest"]["id"].as_str().unwrap();
    cli(&["checkpoint", "verify", id, "--repo", rp, "--json"]);
    assert_eq!(std::fs::read(&index).unwrap(), before);
    let restored = d.path().join("unmerged-restored");
    let rp2 = restored.to_str().unwrap();
    cli(&[
        "checkpoint",
        "restore",
        id,
        "--repo",
        rp,
        "--to",
        rp2,
        "--yes",
        "--accept-incomplete",
        "--json",
    ]);
    let restored_index = git(&restored, &["rev-parse", "--git-path", "index"]);
    assert_eq!(std::fs::read(restored_index).unwrap(), before);
    assert_eq!(
        git(&restored, &["ls-files", "--stage"]),
        git(&work, &["ls-files", "--stage"])
    );
    assert!(!restored.join("delete.txt").exists());
    std::fs::write(work.join("huge.bin"), vec![42; 1_048_577]).unwrap();
    let fail = run(
        &[
            "checkpoint",
            "create",
            "--repo",
            rp,
            "--workspace",
            wid,
            "--include-untracked",
            "--json",
        ],
        &[],
    );
    assert!(!fail.status.success());
    assert_eq!(
        std::fs::metadata(work.join("huge.bin")).unwrap().len(),
        1_048_577
    );
    assert_eq!(std::fs::read(&index).unwrap(), before);
    let list = cli(&["checkpoint", "list", "--repo", rp, "--json"]);
    assert_eq!(list["content_checkpoints"].as_array().unwrap().len(), 1);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn workspace_intent_survives_killed_rit_and_repair_only_finalizes_matching_metadata() {
    use std::{
        io::Write,
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("base"), "base").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let path = d.path().join("ready");
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        path.to_str().unwrap(),
        "--name",
        "ready",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    let listed = cli(&["workspace", "operations", "--repo", rp, "--json"]);
    let entry = &listed["operations"][0];
    assert_eq!(entry["operation"]["workspace"], wid);
    assert_eq!(entry["observation"], "completed");
    let mut op = entry["operation"].clone();
    op["state"] = "worktree_verified".into();
    let refname = format!("refs/rit/operations/{}", op["id"].as_str().unwrap());
    let previous = entry["object"]["hex"].as_str().unwrap();
    let mut p = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    p.stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&op).unwrap().as_bytes())
        .unwrap();
    let out = p.wait_with_output().unwrap();
    assert!(out.status.success());
    let oid = String::from_utf8(out.stdout).unwrap();
    git(&["update-ref", &refname, oid.trim(), previous]);
    let repair = cli(&["workspace", "reconcile", "--repo", rp, "--repair", "--json"]);
    assert_eq!(repair["metadata_repairs"], 1);
    assert_eq!(std::fs::read_to_string(path.join("base")).unwrap(), "base");
    let wrapper = d.path().join("git-kill");
    std::fs::write(&wrapper,"#!/bin/sh\ncase \"$*\" in *\"worktree add --no-checkout\"*) kill -KILL \"$PPID\"; exit 97;; esac\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let interrupted = d.path().join("interrupted");
    let out = Command::new(env!("CARGO_BIN_EXE_rit"))
        .args([
            "--git-executable",
            wrapper.to_str().unwrap(),
            "workspace",
            "create",
            "--repo",
            rp,
            "--path",
            interrupted.to_str().unwrap(),
            "--name",
            "interrupted",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let ops = cli(&["workspace", "operations", "--repo", rp, "--json"]);
    let item = ops["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["operation"]["path"] == interrupted.to_str().unwrap())
        .unwrap();
    assert_eq!(item["operation"]["state"], "path_reserved");
    assert_eq!(item["observation"], "worktree_missing_or_incomplete");
    let branch = item["operation"]["branch"].as_str().unwrap();
    git(&["rev-parse", "--verify", branch]);
    assert!(interrupted.is_dir());
    let repaired = cli(&["workspace", "reconcile", "--repo", rp, "--repair", "--json"]);
    assert_eq!(repaired["metadata_repairs"], 0);
    assert!(interrupted.exists());
    assert_eq!(std::fs::read_to_string(path.join("base")).unwrap(), "base");
    git(&["rev-parse", "--verify", branch]);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn checkpoint_recovers_captured_subset_to_new_worktree_without_overwriting_newer_source() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join(".gitignore"), "ignored\n").unwrap();
    std::fs::write(repo.join("tracked"), "base\n").unwrap();
    std::fs::write(repo.join("deleted"), "base\n").unwrap();
    std::fs::write(repo.join("script"), "script\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let work = d.path().join("source");
    let wp = work.to_str().unwrap();
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let workspace = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        wp,
        "--name",
        "source",
        "--json",
    ]);
    let wid = workspace["workspace"]["id"].as_str().unwrap();
    std::fs::write(work.join("tracked"), "staged\n").unwrap();
    git(&work, &["add", "tracked"]);
    std::fs::write(work.join("tracked"), "unstaged\n").unwrap();
    std::fs::remove_file(work.join("deleted")).unwrap();
    std::fs::write(work.join("new"), "untracked\n").unwrap();
    std::fs::write(work.join("ignored"), "ignored\n").unwrap();
    symlink("tracked", work.join("link")).unwrap();
    std::fs::set_permissions(work.join("script"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let index = git(&work, &["rev-parse", "--git-path", "index"]);
    let captured_index = std::fs::read(&index).unwrap();
    let checkpoint = cli(&[
        "checkpoint",
        "create",
        "--repo",
        rp,
        "--workspace",
        wid,
        "--include-untracked",
        "--include-ignored",
        "ignored",
        "--json",
    ]);
    let id = checkpoint["checkpoint"]["manifest"]["id"].as_str().unwrap();
    std::fs::write(work.join("tracked"), "newer source\n").unwrap();
    std::fs::write(work.join("new"), "newer untracked\n").unwrap();
    let newer_index = std::fs::read(&index).unwrap();
    let marker = d.path().join("restore-hook-ran");
    for hookname in ["reference-transaction", "post-checkout"] {
        let hook = repo.join(".git/hooks").join(hookname);
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let dest = d.path().join("restore");
    let dests = dest.to_str().unwrap();
    let preview = cli(&[
        "checkpoint",
        "restore",
        id,
        "--repo",
        rp,
        "--to",
        dests,
        "--preview",
        "--json",
    ]);
    assert_eq!(preview["worktree_or_refs_modified"], false);
    assert!(!dest.exists());
    assert!(
        preview["plan"]["create_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "new")
    );
    let no_confirmation = run(
        &[
            "checkpoint",
            "restore",
            id,
            "--repo",
            rp,
            "--to",
            dests,
            "--json",
        ],
        &[],
    );
    assert!(!no_confirmation.status.success());
    assert!(!dest.exists());
    let out = cli(&[
        "checkpoint",
        "restore",
        id,
        "--repo",
        rp,
        "--to",
        dests,
        "--yes",
        "--accept-incomplete",
        "--json",
    ]);
    assert_eq!(out["complete"], false);
    assert_eq!(out["restore"]["observation"], "completed_inventory_only");
    assert_eq!(
        std::fs::read_to_string(dest.join("tracked")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(git(&dest, &["show", ":tracked"]), "staged");
    assert_eq!(
        std::fs::read_to_string(dest.join("new")).unwrap(),
        "untracked\n"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("ignored")).unwrap(),
        "ignored\n"
    );
    assert!(!dest.join("deleted").exists());
    assert_eq!(
        std::fs::read_link(dest.join("link")).unwrap(),
        std::path::PathBuf::from("tracked")
    );
    assert_ne!(
        std::fs::metadata(dest.join("script"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    let restored_index = git(&dest, &["rev-parse", "--git-path", "index"]);
    assert_eq!(std::fs::read(restored_index).unwrap(), captured_index);
    assert_eq!(
        std::fs::read_to_string(work.join("tracked")).unwrap(),
        "newer source\n"
    );
    assert_eq!(
        std::fs::read_to_string(work.join("new")).unwrap(),
        "newer untracked\n"
    );
    assert_eq!(std::fs::read(&index).unwrap(), newer_index);
    let exists = run(
        &[
            "checkpoint",
            "restore",
            id,
            "--repo",
            rp,
            "--to",
            wp,
            "--yes",
            "--accept-incomplete",
            "--json",
        ],
        &[],
    );
    assert!(!exists.status.success());
    assert_eq!(
        std::fs::read_to_string(work.join("tracked")).unwrap(),
        "newer source\n"
    );
    assert!(!marker.exists(), "restore executed repository hook");
    let list = cli(&["checkpoint", "restores", "--repo", rp, "--json"]);
    assert_eq!(list["restores"].as_array().unwrap().len(), 1);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn restore_refuses_concurrently_created_destination_file_and_retains_intent() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("tracked"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let source = d.path().join("source");
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        source.to_str().unwrap(),
        "--name",
        "src",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    std::fs::write(source.join("tracked"), "captured\n").unwrap();
    let c = cli(&[
        "checkpoint",
        "create",
        "--repo",
        rp,
        "--workspace",
        wid,
        "--json",
    ]);
    let cid = c["checkpoint"]["manifest"]["id"].as_str().unwrap();
    let dest = d.path().join("restore");
    let wrapper = d.path().join("git-collision");
    std::fs::write(&wrapper,"#!/bin/sh\nif [ -f \"$V5_DEST/.git\" ] && [ ! -f \"$V5_DEST/.inserted\" ]; then printf 'newer concurrent content\\n' > \"$V5_DEST/tracked\"; printf x > \"$V5_DEST/.inserted\"; fi\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .env("V5_DEST", &dest)
        .args([
            "--git-executable",
            wrapper.to_str().unwrap(),
            "checkpoint",
            "restore",
            cid,
            "--repo",
            rp,
            "--to",
            dest.to_str().unwrap(),
            "--yes",
            "--accept-incomplete",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read_to_string(dest.join("tracked")).unwrap(),
        "newer concurrent content\n"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("tracked")).unwrap(),
        "captured\n"
    );
    let list = cli(&["checkpoint", "restores", "--repo", rp, "--json"]);
    assert_eq!(
        list["restores"][0]["observation"],
        "interrupted_manual_recovery_required"
    );
    assert!(
        list["restores"][0]["operation"]["current_path"]
            .as_str()
            .is_some()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn restore_intent_survives_kill_before_worktree_creation_without_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a"), "base\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let source = d.path().join("source");
    let cli = |args: &[&str]| {
        let o = run(args, &[]);
        assert!(o.status.success(), "{args:?}: {}", text(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    let w = cli(&[
        "workspace",
        "create",
        "--repo",
        rp,
        "--path",
        source.to_str().unwrap(),
        "--name",
        "source",
        "--json",
    ]);
    let wid = w["workspace"]["id"].as_str().unwrap();
    std::fs::write(source.join("a"), "important newer data\n").unwrap();
    let c = cli(&[
        "checkpoint",
        "create",
        "--repo",
        rp,
        "--workspace",
        wid,
        "--json",
    ]);
    let cid = c["checkpoint"]["manifest"]["id"].as_str().unwrap();
    let wrapper = d.path().join("git-kill");
    std::fs::write(&wrapper,"#!/bin/sh\ncase \"$*\" in *\"worktree add --no-checkout --detach\"*) kill -KILL \"$PPID\"; exit 97;; esac\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let dest = d.path().join("restore");
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .args([
            "--git-executable",
            wrapper.to_str().unwrap(),
            "checkpoint",
            "restore",
            cid,
            "--repo",
            rp,
            "--to",
            dest.to_str().unwrap(),
            "--yes",
            "--accept-incomplete",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(dest.is_dir());
    let list = cli(&["checkpoint", "restores", "--repo", rp, "--json"]);
    let record = &list["restores"][0];
    assert_eq!(record["operation"]["state"], "directory_created");
    assert_eq!(record["observation"], "worktree_missing_or_incomplete");
    assert_eq!(
        std::fs::read_to_string(source.join("a")).unwrap(),
        "important newer data\n"
    );
    assert!(dest.is_dir());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn checkpoint_refuses_concurrent_file_change_without_reverting_it() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_owned()
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("file"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let rp = repo.to_str().unwrap();
    let w = d.path().join("work");
    let create = run(
        &[
            "workspace",
            "create",
            "--repo",
            rp,
            "--path",
            w.to_str().unwrap(),
            "--name",
            "race",
            "--json",
        ],
        &[],
    );
    assert!(create.status.success());
    let v: serde_json::Value = serde_json::from_slice(&create.stdout).unwrap();
    let id = v["workspace"]["id"].as_str().unwrap();
    std::fs::write(w.join("file"), "first observation\n").unwrap();
    let index = git(&w, &["rev-parse", "--git-path", "index"]);
    let before = std::fs::read(&index).unwrap();
    let wrapper = d.path().join("git-race");
    std::fs::write(&wrapper,"#!/bin/sh\ncase \"$*\" in *\"hash-object --no-filters -w --stdin\"*) n=0; if [ -f \"$V5_COUNTER\" ]; then read -r n < \"$V5_COUNTER\"; fi; n=$((n+1)); printf '%s\\n' \"$n\" > \"$V5_COUNTER\"; if [ \"$n\" = 2 ]; then printf 'newer concurrent content\\n' > \"$V5_FILE\"; fi;; esac\nexec /usr/bin/git \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rit"))
        .env("V5_COUNTER", d.path().join("counter"))
        .env("V5_FILE", w.join("file"))
        .args([
            "--git-executable",
            wrapper.to_str().unwrap(),
            "checkpoint",
            "create",
            "--repo",
            rp,
            "--workspace",
            id,
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("file"));
    assert_eq!(
        std::fs::read_to_string(w.join("file")).unwrap(),
        "newer concurrent content\n"
    );
    assert_eq!(std::fs::read(&index).unwrap(), before);
    let listed = run(&["checkpoint", "list", "--repo", rp, "--json"], &[]);
    assert!(listed.status.success());
    let items: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert!(items["content_checkpoints"].as_array().unwrap().is_empty());
}
