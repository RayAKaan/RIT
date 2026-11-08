use rit_core::repository::{Head, Inspection};
use rit_storage::{Registry, StorageError, diagnose};
use tempfile::TempDir;
fn sample(root: &str, common: &str) -> Inspection {
    Inspection {
        location: root.into(),
        worktree: Some(root.into()),
        git_dir: common.into(),
        common_git_dir: common.into(),
        bare: false,
        object_format: "sha1".into(),
        shallow: false,
        head: Head::Unborn {
            name: "main".into(),
        },
        remotes: vec![],
        status: None,
    }
}
#[test]
fn migration_persistence_idempotency_and_locations() {
    let d = TempDir::new().unwrap();
    assert!(diagnose(d.path()).unwrap().is_none());
    let id;
    {
        let mut db = Registry::open(d.path()).unwrap();
        let (r, created) = db
            .register(&sample("/one", "/common"), Some("My project"))
            .unwrap();
        assert!(created);
        id = r.id;
        let (same, created) = db.register(&sample("/one", "/common"), None).unwrap();
        assert!(!created);
        assert_eq!(same.id, id);
        assert_eq!(same.display_name, "My project");
        let (with_second, created) = db.register(&sample("/two", "/common"), None).unwrap();
        assert!(!created);
        assert_eq!(with_second.id, id);
        assert_eq!(with_second.locations.len(), 2);
        assert!(matches!(
            db.register(&sample("/one", "/different"), None),
            Err(StorageError::LocationConflict)
        ));
        assert_eq!(db.list().unwrap().len(), 1);
        assert!(db.quick_check().unwrap());
    }
    assert_eq!(diagnose(d.path()).unwrap(), Some(1));
    let db = Registry::open(d.path()).unwrap();
    assert_eq!(db.get(id).unwrap().unwrap().locations.len(), 2);
}
#[test]
fn future_version_and_failed_migration_preserve_data() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE keep_me (v TEXT); INSERT INTO keep_me VALUES ('kept'); PRAGMA user_version=99;").unwrap();
    }
    assert!(matches!(
        Registry::open(d.path()),
        Err(StorageError::FutureSchema(99))
    ));
    let c = rusqlite::Connection::open(&p).unwrap();
    let v: String = c
        .query_row("SELECT v FROM keep_me", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "kept");
    drop(c);
    let d = TempDir::new().unwrap();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE repositories (marker TEXT); INSERT INTO repositories VALUES ('kept');",
        )
        .unwrap();
    }
    assert!(Registry::open(d.path()).is_err());
    let c = rusqlite::Connection::open(&p).unwrap();
    let v: String = c
        .query_row("SELECT marker FROM repositories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "kept");
    let ver: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(ver, 0);
}
#[test]
fn foreign_key_enforced_and_corruption_detected() {
    let d = TempDir::new().unwrap();
    let _registry = Registry::open(d.path()).unwrap();
    let conn = rusqlite::Connection::open(d.path().join("rit.sqlite3")).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let err=conn.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES('missing','p','g','worktree',0,0)",[]);
    assert!(err.is_err());
    drop(conn);
    drop(_registry);
    let d = TempDir::new().unwrap();
    std::fs::write(d.path().join("rit.sqlite3"), b"not a sqlite database").unwrap();
    assert!(Registry::open(d.path()).is_err());
    assert_eq!(
        std::fs::read(d.path().join("rit.sqlite3")).unwrap(),
        b"not a sqlite database"
    );
}
#[test]
fn locked_database_returns_an_error_without_losing_rows() {
    let d = TempDir::new().unwrap();
    let mut registry = Registry::open(d.path()).unwrap();
    let (record, _) = registry.register(&sample("/one", "/common"), None).unwrap();
    let c = rusqlite::Connection::open(d.path().join("rit.sqlite3")).unwrap();
    c.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(
        registry
            .register(&sample("/two", "/common2"), None)
            .is_err()
    );
    c.execute_batch("ROLLBACK").unwrap();
    assert_eq!(registry.get(record.id).unwrap().unwrap().locations.len(), 1);
    assert_eq!(registry.list().unwrap().len(), 1);
}
#[cfg(unix)]
#[test]
fn refuses_symlink_database_without_mutating_target() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    let target = d.path().join("victim.sqlite");
    let data = d.path().join("data");
    std::fs::create_dir(&data).unwrap();
    {
        let c = rusqlite::Connection::open(&target).unwrap();
        c.execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');",
        )
        .unwrap();
    }
    symlink(&target, data.join("rit.sqlite3")).unwrap();
    assert!(Registry::open(&data).is_err());
    assert!(diagnose(&data).is_err());
    let c = rusqlite::Connection::open(&target).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='repositories'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}
#[test]
fn rejects_unrelated_unversioned_database_without_mutation() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');",
        )
        .unwrap();
    }
    assert!(matches!(
        Registry::open(d.path()),
        Err(StorageError::UnrecognizedDatabase)
    ));
    let c = rusqlite::Connection::open(p).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
}
#[cfg(unix)]
#[test]
fn refuses_existing_hard_link_to_registry() {
    let d = TempDir::new().unwrap();
    let first = d.path().join("first");
    let alias = d.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&alias).unwrap();
    let mut registry = Registry::open(&first).unwrap();
    registry.register(&sample("/one", "/common"), None).unwrap();
    drop(registry);
    std::fs::hard_link(first.join("rit.sqlite3"), alias.join("rit.sqlite3")).unwrap();
    assert!(matches!(
        Registry::open(&alias),
        Err(StorageError::HardLinkedDatabase)
    ));
    let conn = rusqlite::Connection::open(first.join("rit.sqlite3")).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM repositories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
