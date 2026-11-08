use rit_core::repository::{Head, Inspection};
use rit_storage::{Registry, StorageError, diagnose};
use tempfile::TempDir;
#[cfg(target_os = "linux")]
fn private_temp() -> TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}
#[cfg(not(target_os = "linux"))]
fn private_temp() -> TempDir {
    TempDir::new().unwrap()
}
#[cfg(target_os = "linux")]
fn make_private(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
}
#[cfg(all(unix, not(target_os = "linux")))]
fn make_private(_: &std::path::Path) {}
#[cfg(target_os = "linux")]
fn make_private_file(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
}
#[cfg(not(target_os = "linux"))]
fn make_private_file(_: &std::path::Path) {}
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
    let d = private_temp();
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
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch("CREATE TABLE keep_me (v TEXT); INSERT INTO keep_me VALUES ('kept'); PRAGMA user_version=99;").unwrap();
    }
    make_private_file(&p);
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
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE repositories (marker TEXT); INSERT INTO repositories VALUES ('kept');",
        )
        .unwrap();
    }
    make_private_file(&p);
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
    let d = private_temp();
    let _registry = Registry::open(d.path()).unwrap();
    let conn = rusqlite::Connection::open(d.path().join("rit.sqlite3")).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let err=conn.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES('missing','p','g','worktree',0,0)",[]);
    assert!(err.is_err());
    drop(conn);
    drop(_registry);
    let d = private_temp();
    std::fs::write(d.path().join("rit.sqlite3"), b"not a sqlite database").unwrap();
    assert!(Registry::open(d.path()).is_err());
    assert_eq!(
        std::fs::read(d.path().join("rit.sqlite3")).unwrap(),
        b"not a sqlite database"
    );
}
#[test]
fn locked_database_returns_an_error_without_losing_rows() {
    let d = private_temp();
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
    let d = private_temp();
    let target = d.path().join("victim.sqlite");
    let data = d.path().join("data");
    std::fs::create_dir(&data).unwrap();
    make_private(&data);
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
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');",
        )
        .unwrap();
    }
    make_private_file(&p);
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
    let d = private_temp();
    let first = d.path().join("first");
    let alias = d.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&alias).unwrap();
    make_private(&first);
    make_private(&alias);
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
#[test]
fn marker_and_explicit_legacy_adoption_preserve_records() {
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    let id = {
        let mut r = Registry::open(d.path()).unwrap();
        r.register(&sample("/one", "/common"), Some("legacy"))
            .unwrap()
            .0
            .id
    };
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
                .unwrap(),
            rit_storage::APPLICATION_ID
        );
        c.pragma_update(None, "application_id", 0).unwrap();
    }
    let before = rit_storage::verify(d.path()).unwrap().unwrap();
    assert!(before.valid_legacy);
    assert!(!before.marked_rit);
    assert!(matches!(
        Registry::open(d.path()),
        Err(StorageError::LegacyAdoptionRequired)
    ));
    assert!(matches!(
        rit_storage::adopt_legacy(d.path(), false),
        Err(StorageError::ConfirmationRequired)
    ));
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    rit_storage::adopt_legacy(d.path(), true).unwrap();
    assert!(rit_storage::verify(d.path()).unwrap().unwrap().marked_rit);
    let r = Registry::open(d.path()).unwrap().get(id).unwrap().unwrap();
    assert_eq!(r.display_name, "legacy");
    assert_eq!(r.locations.len(), 1);
}
#[test]
fn rejects_legacy_schema_with_extra_object_and_unknown_application() {
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    drop(Registry::open(d.path()).unwrap());
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.pragma_update(None, "application_id", 0).unwrap();
        c.execute_batch(
            "CREATE TRIGGER unexpected AFTER INSERT ON repositories BEGIN SELECT 1; END;",
        )
        .unwrap();
    }
    let before = std::fs::read(&p).unwrap();
    assert!(!rit_storage::verify(d.path()).unwrap().unwrap().valid_legacy);
    assert!(rit_storage::adopt_legacy(d.path(), true).is_err());
    assert_eq!(before, std::fs::read(&p).unwrap());
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.pragma_update(None, "application_id", 77).unwrap();
    }
    assert!(matches!(
        Registry::open(d.path()),
        Err(StorageError::UnknownApplication(77))
    ));
}
#[cfg(target_os = "linux")]
#[test]
fn private_directory_enforced_and_new_files_are_private() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let d = private_temp();
    let data = d.path().join("data");
    let r = Registry::open(&data).unwrap();
    drop(r);
    assert_eq!(std::fs::metadata(&data).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(data.join("rit.sqlite3")).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(data.join(".rit-storage.lock"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    let unsafe_dir = d.path().join("unsafe");
    std::fs::create_dir(&unsafe_dir).unwrap();
    std::fs::set_permissions(&unsafe_dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Registry::open(&unsafe_dir.join("child")).is_err());
    assert!(!unsafe_dir.join("child").exists());
}
#[cfg(target_os = "linux")]
#[test]
fn rejects_symlink_ancestor_and_sidecar_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let d = private_temp();
    let target = d.path().join("outside");
    std::fs::create_dir(&target).unwrap();
    make_private(&target);
    let alias = d.path().join("alias");
    symlink(&target, &alias).unwrap();
    assert!(Registry::open(&alias.join("data")).is_err());
    assert!(!target.join("data").exists());
    let data = d.path().join("data");
    drop(Registry::open(&data).unwrap());
    let outside = d.path().join("important");
    std::fs::write(&outside, b"unchanged").unwrap();
    symlink(&outside, data.join("rit.sqlite3-journal")).unwrap();
    assert!(Registry::open(&data).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"unchanged");
}
#[test]
fn unknown_empty_database_is_not_automatically_initialized() {
    let d = private_temp();
    let p = d.path().join("rit.sqlite3");
    std::fs::write(&p, []).unwrap();
    make_private_file(&p);
    let before = std::fs::read(&p).unwrap();
    assert!(Registry::open(d.path()).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), before);
}

#[cfg(target_os = "linux")]
#[test]
fn refuses_replaced_marked_database_even_if_schema_and_application_id_match() {
    // Both files are clean SQLite databases and both connections have closed
    // before the copy: this does not copy a live SQLite transaction.
    let d = private_temp();
    let original = d.path().join("original");
    let other = d.path().join("other");
    {
        let mut db = Registry::open(&original).unwrap();
        db.register(&sample("/one", "/one-common"), Some("original"))
            .unwrap();
    }
    {
        let mut db = Registry::open(&other).unwrap();
        db.register(&sample("/two", "/two-common"), Some("unrelated"))
            .unwrap();
    }
    let path = original.join("rit.sqlite3");
    let preserved = original.join("original-preserved.sqlite3");
    std::fs::rename(&path, &preserved).unwrap();
    std::fs::copy(other.join("rit.sqlite3"), &path).unwrap();
    make_private_file(&path);
    let before = std::fs::read(&path).unwrap();
    let result = Registry::open(&original);
    assert!(
        result.is_err(),
        "a substituted database with matching SQLite marker and schema must not be authorized"
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(preserved.exists());
}
