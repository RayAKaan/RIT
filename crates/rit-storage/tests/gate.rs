use rit_storage::{Registry, StorageError};

#[test]
fn write_paths_are_not_security_gate_blocked_and_fail_safely() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("missing");
    // After Phase 2.1 remediation attempt, writable paths use protected directory + manifest binding.
    // Missing dir may be created by secure prepare only under validated rules; registry open will fail
    // safely if it cannot establish a bound manifest/DB. It must not return the old global gate block.
    let r = Registry::open(&missing);
    assert!(!matches!(r, Err(StorageError::SecurityGateBlocked)));
    let a = rit_storage::adopt_legacy(&missing, true);
    assert!(!matches!(a, Err(StorageError::SecurityGateBlocked)));

    let existing = d.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    let file = existing.join("rit.sqlite3");
    std::fs::write(&file, b"unrelated bytes").unwrap();
    let before = std::fs::read(&file).unwrap();
    assert!(Registry::open(&existing).is_err());
    assert!(rit_storage::adopt_legacy(&existing, true).is_err());
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "unrelated database bytes must not be mutated"
    );
}
