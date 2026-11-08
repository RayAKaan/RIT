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
