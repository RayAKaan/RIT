use rit_storage::{Registry, StorageError};
#[test]
fn write_gate_prevents_both_public_paths_without_touching_files() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("missing");
    assert!(matches!(
        Registry::open(&missing),
        Err(StorageError::SecurityGateBlocked)
    ));
    assert!(matches!(
        rit_storage::adopt_legacy(&missing, true),
        Err(StorageError::SecurityGateBlocked)
    ));
    assert!(!missing.exists());
    let existing = d.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    let file = existing.join("rit.sqlite3");
    std::fs::write(&file, b"unrelated bytes").unwrap();
    let before = std::fs::read(&file).unwrap();
    assert!(matches!(
        Registry::open(&existing),
        Err(StorageError::SecurityGateBlocked)
    ));
    assert!(matches!(
        rit_storage::adopt_legacy(&existing, true),
        Err(StorageError::SecurityGateBlocked)
    ));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}
