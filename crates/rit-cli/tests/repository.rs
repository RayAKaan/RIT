use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
fn git(p: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(args)
        .current_dir(p)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(s.success());
}
fn rit(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .env_remove("RIT_GIT_EXECUTABLE")
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env("XDG_CONFIG_HOME", data.join("config"))
        .env("APPDATA", data.join("config"))
        .output()
        .unwrap()
}
fn json(o: &Output) -> serde_json::Value {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}
#[test]
fn cli_register_inspect_status_and_idempotency() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("space ü repo");
    let data = temp.path().join("data");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    fs::write(repo.join("not tracked.txt"), "work").unwrap();
    let p = repo.to_str().unwrap();
    let inspected = json(&rit(&data, &["inspect", p, "--json"]));
    assert_eq!(
        inspected["inspection"]["status"]["changes"][0]["kind"],
        "untracked"
    );
    assert!(!data.exists());
    let one = json(&rit(&data, &["init", p, "--json"]));
    assert_eq!(one["created"], true);
    let id = one["repository"]["id"].as_str().unwrap();
    let two = json(&rit(&data, &["repository", "register", p, "--json"]));
    assert_eq!(two["created"], false);
    assert_eq!(two["repository"]["id"], id);
    let list = json(&rit(&data, &["repository", "list", "--json"]));
    assert_eq!(list["repositories"].as_array().unwrap().len(), 1);
    let info = json(&rit(&data, &["repository", "info", id, "--json"]));
    assert_eq!(info["repository"]["id"], id);
    let stat = json(&rit(&data, &["status", id, "--json"]));
    assert_eq!(stat["status"]["changes"][0]["path"], "not tracked.txt");
    let failed = rit(&data, &["init", temp.path().to_str().unwrap()]);
    assert!(!failed.status.success());
    assert_eq!(
        json(&rit(&data, &["repository", "list", "--json"]))["repositories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(!repo.join(".rit").exists());
    assert!(!repo.join("rit.sqlite3").exists());
}
#[test]
fn rejects_database_inside_repository() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let data = repo.join("hidden-data");
    let out = rit(&data, &["init", repo.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!data.exists());
}
#[test]
fn linked_worktrees_share_one_persistent_identity() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    let linked = temp.path().join("linked");
    let data = temp.path().join("data");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(
        &root,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let a = json(&rit(&data, &["init", root.to_str().unwrap(), "--json"]));
    let b = json(&rit(&data, &["init", linked.to_str().unwrap(), "--json"]));
    assert_eq!(a["repository"]["id"], b["repository"]["id"]);
    assert_eq!(b["repository"]["locations"].as_array().unwrap().len(), 2);
}
#[test]
fn ignores_inherited_git_redirection_and_keeps_urls_secret() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    let data = temp.path().join("data");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://name:fake-secret@example.invalid/repo",
        ],
    );
    let out = Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(&data)
        .args(["inspect", root.to_str().unwrap(), "--json"])
        .env("GIT_DIR", temp.path().join("nonexistent"))
        .env("GIT_WORK_TREE", temp.path())
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env_remove("RIT_GIT_EXECUTABLE")
        .output()
        .unwrap();
    let result = json(&out);
    assert_eq!(result["inspection"]["remotes"][0], "origin");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-secret"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("fake-secret"));
}
#[test]
fn inspection_and_registration_do_not_modify_git_metadata() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    let data = temp.path().join("data");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    fs::write(root.join("tracked"), "hello").unwrap();
    git(&root, &["add", "tracked"]);
    git(
        &root,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    fs::write(root.join("tracked"), "edited").unwrap();
    let paths = [
        root.join(".git/HEAD"),
        root.join(".git/index"),
        root.join(".git/config"),
        root.join("tracked"),
    ];
    let before: Vec<Vec<u8>> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    assert!(
        rit(&data, &["inspect", root.to_str().unwrap(), "--json"])
            .status
            .success()
    );
    assert!(
        rit(&data, &["init", root.to_str().unwrap(), "--json"])
            .status
            .success()
    );
    let after: Vec<Vec<u8>> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    assert_eq!(before, after);
}
#[test]
fn explicit_git_override_wins_and_doctor_reports_missing_git() {
    let temp = TempDir::new().unwrap();
    let data = temp.path().join("data");
    let out = Command::new(env!("CARGO_BIN_EXE_rit"))
        .arg("--data-dir")
        .arg(&data)
        .args([
            "--git-executable",
            "missing-git-absolutely",
            "doctor",
            "--json",
        ])
        .env("RIT_GIT_EXECUTABLE", "git")
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["git"]["status"], "error");
    assert!(!data.exists());
}
#[test]
fn stale_registration_is_preserved() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let data = temp.path().join("data");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let result = json(&rit(&data, &["init", repo.to_str().unwrap(), "--json"]));
    let id = result["repository"]["id"].as_str().unwrap();
    fs::remove_dir_all(&repo).unwrap();
    let info = rit(&data, &["repository", "info", id, "--json"]);
    assert!(!info.status.success());
    assert!(String::from_utf8_lossy(&info.stderr).contains("unavailable"));
    assert_eq!(
        json(&rit(&data, &["repository", "list", "--json"]))["repositories"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn listing_uninitialized_registry_does_not_write_into_repository() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let data = repo.join("should-not-exist");
    let result = json(&rit(&data, &["repository", "list", "--json"]));
    assert!(result["repositories"].as_array().unwrap().is_empty());
    assert!(!data.exists());
}
#[test]
fn listing_marks_a_removed_location_missing() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let data = temp.path().join("data");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let result = json(&rit(&data, &["init", repo.to_str().unwrap(), "--json"]));
    assert!(result["created"].as_bool().unwrap());
    let before = json(&rit(&data, &["repository", "list", "--json"]));
    assert_eq!(
        before["repositories"][0]["location_availability"][0]["state"],
        "present_unverified"
    );
    fs::remove_dir_all(&repo).unwrap();
    let after = json(&rit(&data, &["repository", "list", "--json"]));
    assert_eq!(
        after["repositories"][0]["location_availability"][0]["state"],
        "missing"
    );
}
#[test]
fn repurposed_first_location_does_not_hide_valid_linked_worktree() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    let linked = temp.path().join("linked");
    let data = temp.path().join("data");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(
        &root,
        &[
            "-c",
            "user.name=x",
            "-c",
            "user.email=x@y",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "first",
        ],
    );
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let first = json(&rit(&data, &["init", linked.to_str().unwrap(), "--json"]));
    let id = first["repository"]["id"].as_str().unwrap();
    json(&rit(&data, &["init", root.to_str().unwrap(), "--json"]));
    fs::remove_dir_all(&linked).unwrap();
    fs::create_dir(&linked).unwrap();
    git(&linked, &["init", "-q"]);
    let info = json(&rit(&data, &["repository", "info", id, "--json"]));
    let loc = info["inspection"]["location"].as_str().unwrap();
    let expected = root.canonicalize().unwrap().to_string_lossy().into_owned();
    assert_eq!(loc, expected);
    assert_eq!(info["repository"]["id"], id);
}
#[test]
fn concurrent_initialization_and_same_repository_registration_are_idempotent() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let data = temp.path().join("data");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let mut children = Vec::new();
    for _ in 0..8 {
        children.push(
            Command::new(env!("CARGO_BIN_EXE_rit"))
                .arg("--data-dir")
                .arg(&data)
                .args(["init", repo.to_str().unwrap(), "--json"])
                .env_remove("RIT_GIT_EXECUTABLE")
                .env_remove("RIT_LOG_LEVEL")
                .env_remove("RIT_LOG_FORMAT")
                .env("XDG_CONFIG_HOME", temp.path().join("config"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let mut ids = Vec::new();
    let mut created = 0;
    let mut rejected_race = false;
    for child in children {
        let out = child.wait_with_output().unwrap();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("refusing to initialize a non-empty unrecognized SQLite database")
                    || stderr.contains("registry schema or contents are not recognized")
                    || stderr.contains("database is locked"),
                "{stderr}"
            );
            rejected_race = true;
            continue;
        }
        let result = json(&out);
        ids.push(result["repository"]["id"].as_str().unwrap().to_owned());
        created += usize::from(result["created"].as_bool().unwrap());
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 1);
    assert_eq!(created, 1);
    let listing = json(&rit(&data, &["repository", "list", "--json"]));
    if cfg!(any(target_os = "linux", target_os = "windows")) || !rejected_race {
        assert_eq!(listing["repositories"].as_array().unwrap().len(), 1);
    } else {
        assert_eq!(
            listing["repositories"].as_array().unwrap().len(),
            1,
            "{listing}"
        );
    }
}
#[cfg(unix)]
#[test]
fn valid_git_environment_override_is_used() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let exe = Command::new("git").arg("--exec-path").output().unwrap();
    assert!(exe.status.success());
    // An absolute executable name makes this test independent of relative PATH entries.
    let git_path = Command::new("which").arg("git").output().unwrap();
    assert!(git_path.status.success());
    let git_path = String::from_utf8(git_path.stdout).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rit"))
        .args(["inspect", repo.to_str().unwrap(), "--json"])
        .env("RIT_GIT_EXECUTABLE", git_path.trim())
        .env_remove("RIT_LOG_LEVEL")
        .env_remove("RIT_LOG_FORMAT")
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .output()
        .unwrap();
    assert_eq!(
        json(&out)["inspection"]["location"],
        repo.canonicalize().unwrap().to_string_lossy().as_ref()
    );
}
#[test]
fn rejects_data_directory_which_contains_repository() {
    let temp = TempDir::new().unwrap();
    let data = temp.path().join("data");
    let repo = data.join("repo");
    fs::create_dir(&data).unwrap();
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let out = rit(&data, &["init", repo.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!data.join("rit.sqlite3").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_never_calls_schema_readability_security_health() {
    let d = TempDir::new().unwrap();
    let root = d.path().join("repo");
    let data = d.path().join("data");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    json(&rit(&data, &["init", root.to_str().unwrap(), "--json"]));
    let report = json(&rit(&data, &["doctor", "--json"]));
    assert_eq!(
        report["storage"]["status"],
        "schema_readable_security_unverified"
    );
    assert_eq!(report["overall_status"], "security_blocked");
    assert_eq!(report["security_gate"], "blocked_phase_2_1");
}
