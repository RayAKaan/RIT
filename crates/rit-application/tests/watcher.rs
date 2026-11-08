#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn bounded_queue_reports_overflow_and_restart_reconciles() {
    use std::{os::unix::fs::PermissionsExt, process::Command, time::Duration};
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let r = d.path().join("repo");
    std::fs::create_dir(&r).unwrap();
    let git = |args: &[&str]| {
        let s = Command::new("git")
            .args(args)
            .current_dir(&r)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .status()
            .unwrap();
        assert!(s.success());
    };
    git(&["init", "-q"]);
    std::fs::write(r.join("base"), "base\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let g = rit_git::Git::discover(None).unwrap();
    let service = rit_application::WorkspaceService::new(g.clone());
    let w = d.path().join("workspace");
    let m = service.create(&r, &w, "one", "HEAD").unwrap();
    assert_eq!(m.state, "ready");
    let mut watcher = rit_application::watcher::ActivityWatcher::start(g, &r, 1).unwrap();
    for i in 0..300 {
        std::fs::write(w.join(format!("burst{i}")), "x").unwrap();
    }
    std::thread::sleep(Duration::from_millis(300));
    let batch = watcher.poll(Duration::from_secs(1));
    assert!(batch.overflowed, "bounded channel should report loss");
    watcher.restart().unwrap();
    std::fs::write(w.join("after_restart"), "changed").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut seen = false;
    while std::time::Instant::now() < deadline {
        let batch = watcher.poll(Duration::from_millis(200));
        if batch.events.iter().any(|e| {
            e.path == "after_restart" && e.workspace_id.as_deref() == Some(&m.id.to_string())
        }) {
            seen = true;
            break;
        }
    }
    assert!(seen, "watch did not resume after restart");
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn git_snapshot_transitions_use_real_index_head_and_lock_evidence() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("a.py"), "x=0\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let provider = rit_git::Git::discover(None).unwrap();
    let work = d.path().join("work");
    let ws = rit_application::WorkspaceService::new(provider.clone());
    let manifest = ws.create(&repo, &work, "events", "HEAD").unwrap();
    let service = rit_application::GitReadService::new(provider);
    let before = service.activity_snapshot(&repo).unwrap();
    std::fs::write(work.join("a.py"), "x=1\n").unwrap();
    git(&work, &["add", "a.py"]);
    let staged = service.activity_snapshot(&repo).unwrap();
    let delta = rit_application::activity::compare_snapshots(&before, &staged);
    assert!(
        delta
            .transitions
            .iter()
            .any(|t| t.kind == "index_status_changed"
                && t.path.as_deref() == Some("a.py")
                && t.after.as_deref() == Some("M"))
    );
    assert!(!delta.complete_event_history);
    git(&work, &["commit", "-qm", "change"]);
    let committed = service.activity_snapshot(&repo).unwrap();
    let delta = rit_application::activity::compare_snapshots(&staged, &committed);
    assert!(
        delta
            .transitions
            .iter()
            .any(|t| t.kind == "head_commit_changed" && t.before != t.after)
    );
    assert!(
        delta
            .transitions
            .iter()
            .any(|t| t.kind == "index_status_changed"
                && t.path.as_deref() == Some("a.py")
                && t.after.is_none())
    );
    ws.lock(&repo, manifest.id, Some("test")).unwrap();
    let locked = service.activity_snapshot(&repo).unwrap();
    let delta = rit_application::activity::compare_snapshots(&committed, &locked);
    assert!(
        delta
            .transitions
            .iter()
            .any(|t| t.kind == "worktree_lock_changed" && t.after.as_deref() == Some("true"))
    );
    ws.lock(&repo, manifest.id, None).unwrap();
    let mut unknown = service.activity_snapshot(&repo).unwrap();
    for entry in &mut unknown.worktrees {
        if entry.worktree.path == work.to_str().unwrap() {
            entry.status = None;
        }
    }
    let delta = rit_application::activity::compare_snapshots(&staged, &unknown);
    assert!(
        !delta
            .transitions
            .iter()
            .any(|t| t.kind == "index_status_changed")
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn restart_discards_old_full_queue_and_explicitly_reports_history_loss() {
    use std::{
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{Duration, Instant},
    };
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let s = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .status()
            .unwrap();
        assert!(s.success())
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a"), "base").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let provider = rit_git::Git::discover(None).unwrap();
    let work = d.path().join("work");
    let manifest = rit_application::WorkspaceService::new(provider.clone())
        .create(&repo, &work, "watch", "HEAD")
        .unwrap();
    let mut watcher = rit_application::watcher::ActivityWatcher::start(provider, &repo, 1).unwrap();
    for i in 0..5 {
        std::fs::write(work.join(format!("old-{i}")), "old").unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    watcher.restart().unwrap();
    std::fs::write(work.join("fresh"), "new").unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut saw_fresh = false;
    let mut saw_loss = false;
    while Instant::now() < deadline {
        let b = watcher.poll(Duration::from_millis(200));
        saw_loss |= b.overflowed;
        saw_fresh |= b.events.iter().any(|e| {
            e.path == "fresh" && e.workspace_id.as_deref() == Some(manifest.id.to_string().as_str())
        });
        if saw_loss && saw_fresh {
            break;
        }
    }
    assert!(saw_loss, "restart must disclose the observation gap");
    assert!(
        saw_fresh,
        "stale queued events must not occupy the new generation"
    );
}
