//! Real subprocess crash recovery acceptance; failures must remain visible in the required suite.
use rit_core::repository::{Head, Inspection};
use rit_storage::Registry;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::Duration,
};
#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
#[cfg(unix)] use std::os::unix::process::ExitStatusExt;

fn private_temp() -> tempfile::TempDir {
    #[cfg(unix)]
    { tempfile::Builder::new().permissions(std::fs::Permissions::from_mode(0o700)).tempdir().unwrap() }
    #[cfg(not(unix))]
    { tempfile::tempdir().unwrap() }
}

#[test]
fn crash_writer_helper() {
    let Some(path) = std::env::var_os("RIT_TEST_CRASH_DB") else {
        return;
    };
    let c = rusqlite::Connection::open(path).unwrap();
    let version: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0)).unwrap();
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "delete", "fixture requires rollback journal mode");
    println!("RIT_FIXTURE_SQLITE={version} JOURNAL_MODE={mode}");
    c.execute_batch("PRAGMA cache_size=10; BEGIN IMMEDIATE;").unwrap();
    let id: String = c.query_row("SELECT id FROM repositories LIMIT 1", [], |r| r.get(0)).unwrap();
    for n in 0..2000 {
        c.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES(?1,?2,'/g','worktree',0,0)",rusqlite::params![id,format!("/path{n}{}","x".repeat(1000))]).unwrap();
    }
    println!("RIT_WRITER_READY");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(60));
}
#[test]
fn marked_registry_recovers_hot_journal_after_killed_writer() {
    let d = private_temp();
    let mut registry = Registry::open(d.path()).unwrap();
    let observation = Inspection {
        location: "/one".into(), worktree: Some("/one".into()), git_dir: "/common".into(),
        common_git_dir: "/common".into(), bare: false, object_format: "sha1".into(), shallow: false,
        head: Head::Unborn { name: "main".into() }, remotes: vec![], status: None,
    };
    let id = registry.register(&observation, None).unwrap().0.id;
    let mut second = observation.clone();
    second.location = "/two".into(); second.common_git_dir = "/common2".into(); second.git_dir = "/common2".into();
    let id2 = registry.register(&second, None).unwrap().0.id;
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
        assert!(lines.read_line(&mut line).unwrap() > 0, "writer failed before reaching the transaction");
        eprint!("{line}");
        if line.contains("RIT_WRITER_READY") { break; }
    }
    child.kill().unwrap();
    let exit = child.wait().unwrap();
    #[cfg(unix)] { eprintln!("RIT_FIXTURE_WRITER_EXIT_SIGNAL={:?}", exit.signal()); assert_eq!(exit.signal(), Some(9), "fixture must end in SIGKILL"); }
    #[cfg(not(unix))] { eprintln!("RIT_FIXTURE_WRITER_EXIT={exit}"); assert!(!exit.success(), "fixture must terminate abnormally"); }
    let journal = std::fs::read(d.path().join("rit.sqlite3-journal")).unwrap();
    assert!(journal.len() > 512, "journal must exceed SQLite's hot-journal size threshold");
    assert!(path.exists());
    eprintln!("RIT_FIXTURE_JOURNAL_BYTES={} MAGIC={:02x?}", journal.len(), &journal[..8]);
    #[cfg(target_os = "linux")]
    {
        let fs = Command::new("stat").args(["-f", "-c", "%T"]).arg(d.path()).output().unwrap();
        assert!(fs.status.success());
        eprintln!("RIT_FIXTURE_FILESYSTEM={}", String::from_utf8_lossy(&fs.stdout).trim());
    }
    assert_eq!(&journal[..8], &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7], "fixture must have a hot journal");
    // Acceptance criterion: SQLite must recover safely, then return only committed data.
    let registry = Registry::open(d.path()).expect("hot-journal recovery should succeed without adopting an unrelated database");
    assert_eq!(registry.get(id).unwrap().unwrap().locations.len(), 1);
    assert_eq!(registry.get(id2).unwrap().unwrap().locations.len(), 1);
    assert_eq!(registry.list().unwrap().len(), 2, "uncommitted inserts were rolled back");
    assert!(registry.quick_check().unwrap());
    drop(registry);
    let mut registry = Registry::open(d.path()).unwrap();
    let mut third = observation;
    third.location = "/three".into(); third.common_git_dir = "/common3".into(); third.git_dir = "/common3".into();
    let id3 = registry.register(&third, None).unwrap().0.id;
    assert!(registry.get(id3).unwrap().is_some());
    drop(registry);
    let reopened = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "clean_restart_helper", "--nocapture"])
        .env("RIT_TEST_REOPEN_DIR", d.path())
        .status()
        .unwrap();
    assert!(reopened.success(), "separate clean restart must succeed");
}

#[test]
fn clean_restart_helper() {
    let Some(dir) = std::env::var_os("RIT_TEST_REOPEN_DIR") else { return; };
    let path = std::path::Path::new(&dir);
    let registry = Registry::open(path).unwrap();
    assert_eq!(registry.list().unwrap().len(), 3);
    assert!(registry.quick_check().unwrap());
    drop(registry);
    let c = rusqlite::Connection::open_with_flags(path.join("rit.sqlite3"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let result: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(result, "ok");
    let mut stmt = c.prepare("PRAGMA foreign_key_check").unwrap();
    assert!(stmt.query([]).unwrap().next().unwrap().is_none());
}
