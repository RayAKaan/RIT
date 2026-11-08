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
