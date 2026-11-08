//! A deliberately unpassed acceptance test retained for the Phase 2.1 recovery blocker.
#![cfg(target_os = "linux")]
use rit_core::repository::{Head, Inspection};
use rit_storage::Registry;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
fn crash_writer_helper() {
    let Some(path) = std::env::var_os("RIT_TEST_CRASH_DB") else {
        return;
    };
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("PRAGMA cache_size=10; BEGIN IMMEDIATE;")
        .unwrap();
    let id: String = c
        .query_row("SELECT id FROM repositories LIMIT 1", [], |r| r.get(0))
        .unwrap();
    for n in 0..2000 {
        c.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES(?1,?2,'/g','worktree',0,0)",rusqlite::params![id,format!("/path{n}{}","x".repeat(1000))]).unwrap();
    }
    println!("RIT_WRITER_READY");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(60));
}
#[test]
#[ignore = "known critical blocker: read-only identity preflight prevents hot-journal recovery"]
fn marked_registry_recovers_hot_journal_after_killed_writer() {
    let d = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let mut registry = Registry::open(d.path()).unwrap();
    let observation = Inspection {
        location: "/one".into(),
        worktree: Some("/one".into()),
        git_dir: "/common".into(),
        common_git_dir: "/common".into(),
        bare: false,
        object_format: "sha1".into(),
        shallow: false,
        head: Head::Unborn {
            name: "main".into(),
        },
        remotes: vec![],
        status: None,
    };
    let id = registry.register(&observation, None).unwrap().0.id;
    drop(registry);
    let path = d.path().join("rit.sqlite3");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_writer_helper", "--nocapture"])
        .env("RIT_TEST_CRASH_DB", &path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            lines.read_line(&mut line).unwrap() > 0,
            "writer failed before reaching the transaction"
        );
        if line.contains("RIT_WRITER_READY") {
            break;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let journal = std::fs::read(d.path().join("rit.sqlite3-journal")).unwrap();
    assert_eq!(
        &journal[..8],
        &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7],
        "fixture must have a hot journal"
    );
    // Acceptance criterion: SQLite must recover safely, then return only committed data.
    let registry = Registry::open(d.path())
        .expect("hot-journal recovery should succeed without adopting an unrelated database");
    assert_eq!(registry.get(id).unwrap().unwrap().locations.len(), 1);
}
