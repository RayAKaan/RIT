//! SQLite-backed local registry of repositories, not source history.
#![deny(unsafe_code)]
mod manifest;
mod secure_dir;
#[cfg(target_os = "windows")]
mod secure_dir_windows;
use rit_core::repository::{Inspection, Location, Repository, RepositoryId};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("writable registry unavailable: Phase 2.1 storage security gate has not passed")]
    SecurityGateBlocked,
    #[error("unsafe RIT data directory: {0}")]
    UnsafeDirectory(&'static str),
    #[error("cannot prepare data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite registry error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("unsupported registry schema version {0}; upgrade RIT")]
    FutureSchema(i64),
    #[error("refusing database with multiple hard links")]
    HardLinkedDatabase,
    #[error("refusing to initialize a non-empty unrecognized SQLite database")]
    UnrecognizedDatabase,
    #[error(
        "unmarked legacy registry; adoption is unavailable pending safe backup and identity validation"
    )]
    LegacyAdoptionRequired,
    #[error("unrecognized SQLite application ID {0:#x}")]
    UnknownApplication(i64),
    #[error(
        "storage recovery is blocked by a hot journal; preserve the database and journal together and consult the Phase 2.1 report"
    )]
    RecoveryBlocked,
    #[error("registry schema or contents are not recognized; no adoption performed")]
    BadSchema,
    #[error("legacy adoption requires --confirm after verifying a backup")]
    ConfirmationRequired,
    #[error("system clock is before Unix epoch or out of range")]
    Clock,
    #[error("invalid repository identifier stored in registry")]
    BadId,
    #[error("data path is not valid Unicode")]
    NonUnicode,
    #[error("no platform data directory is available; supply --data-dir")]
    NoDataDir,
    #[error(
        "repository location belongs to another registered repository; relocation requires manual review"
    )]
    LocationConflict,
    #[error("registry schema requires migration; run a registration after backing up")]
    SchemaNeedsMigration,
    #[error("repository display name must not be empty")]
    InvalidName,
}

pub fn data_dir(explicit: Option<&Path>) -> Result<PathBuf, StorageError> {
    if let Some(p) = explicit {
        return Ok(p.to_owned());
    }
    if let Some(p) = std::env::var_os("RIT_DATA_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|v| PathBuf::from(v).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share")));
    Ok(base.ok_or(StorageError::NoDataDir)?.join("rit"))
}
pub const APPLICATION_ID: i64 = 0x5249_5431;
const REPOSITORIES_SQL: &str = "CREATE TABLE repositories (id TEXT PRIMARY KEY NOT NULL, common_git_dir TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL CHECK(length(trim(display_name))>0), object_format TEXT NOT NULL, is_bare INTEGER NOT NULL CHECK(is_bare IN (0,1)), created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)";
const LOCATIONS_SQL: &str = "CREATE TABLE repository_locations (id INTEGER PRIMARY KEY, repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE, path TEXT NOT NULL UNIQUE, git_dir TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('worktree','bare')), first_seen_at INTEGER NOT NULL, last_seen_at INTEGER NOT NULL)";
fn identity(c: &Connection) -> Result<(i64, i64), StorageError> {
    Ok((
        c.pragma_query_value(None, "application_id", |r| r.get(0))?,
        c.pragma_query_value(None, "user_version", |r| r.get(0))?,
    ))
}
fn validate_schema(c: &Connection) -> Result<(), StorageError> {
    let mut stmt = c.prepare(
        "SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected_base = [
        ("table", "repositories", REPOSITORIES_SQL),
        ("table", "repository_locations", LOCATIONS_SQL),
    ];
    let expected_id = [
        ("table", "repositories", REPOSITORIES_SQL),
        ("table", "repository_locations", LOCATIONS_SQL),
        (
            "table",
            "rit_storage_identity",
            manifest::IDENTITY_TABLE_SQL,
        ),
    ];
    let matches = |expected: &[(&str, &str, &str)]| {
        rows.len() == expected.len()
            && rows
                .iter()
                .zip(expected)
                .all(|((k, n, s), (ek, en, es))| k == ek && n == en && s.as_deref() == Some(*es))
    };
    if !(matches(&expected_base) || matches(&expected_id)) {
        return Err(StorageError::BadSchema);
    }
    let ok: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(StorageError::BadSchema);
    }
    let mut fk = c.prepare("PRAGMA foreign_key_check")?;
    if fk.query([])?.next()?.is_some() {
        return Err(StorageError::BadSchema);
    }
    let mut ids = c.prepare("SELECT id FROM repositories")?;
    for id in ids.query_map([], |r| r.get::<_, String>(0))? {
        id?.parse::<RepositoryId>()
            .map_err(|_| StorageError::BadId)?;
    }
    Ok(())
}
fn validate_marked(c: &Connection) -> Result<(), StorageError> {
    let (app, version) = identity(c)?;
    if version > 1 {
        return Err(StorageError::FutureSchema(version));
    }
    if app == 0 && version == 1 {
        return Err(StorageError::LegacyAdoptionRequired);
    }
    if app != APPLICATION_ID {
        return if app == 0 {
            Err(StorageError::UnrecognizedDatabase)
        } else {
            Err(StorageError::UnknownApplication(app))
        };
    }
    if version != 1 {
        return Err(StorageError::BadSchema);
    }
    validate_schema(c)
}
fn preflight<T>(result: Result<T, StorageError>) -> Result<T, StorageError> {
    match result {
        Err(StorageError::Sql(rusqlite::Error::SqliteFailure(ref error, _)))
            if error.extended_code == rusqlite::ffi::SQLITE_READONLY_ROLLBACK =>
        {
            Err(StorageError::RecoveryBlocked)
        }
        other => other,
    }
}
fn readonly(path: &Path) -> Result<Connection, StorageError> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    c.busy_timeout(Duration::from_secs(5))?;
    Ok(c)
}
#[derive(Debug)]
pub struct StorageVerification {
    pub application_id: i64,
    pub user_version: i64,
    pub marked_rit: bool,
    pub valid_legacy: bool,
}
pub fn verify(dir: &Path) -> Result<Option<StorageVerification>, StorageError> {
    let Some(guard) = secure_dir::prepare(dir, false)? else {
        return Ok(None);
    };
    let p = guard.path.join("rit.sqlite3");
    if !p.exists() {
        return Ok(None);
    }
    let c = readonly(&p)?;
    let (app, version) = preflight(identity(&c))?;
    let valid = if version == 1 {
        match preflight(validate_schema(&c)) {
            Ok(()) => true,
            Err(StorageError::BadSchema) => false,
            Err(e) => return Err(e),
        }
    } else {
        false
    };
    Ok(Some(StorageVerification {
        application_id: app,
        user_version: version,
        marked_rit: app == APPLICATION_ID && valid,
        valid_legacy: app == 0 && valid,
    }))
}
pub fn adopt_legacy(dir: &Path, confirmed: bool) -> Result<(), StorageError> {
    if !confirmed {
        return Err(StorageError::ConfirmationRequired);
    }
    let directory = secure_dir::prepare(dir, false)?.ok_or(StorageError::UnrecognizedDatabase)?;
    let _lock = directory.lock_initialization()?;
    let path = directory.path.join("rit.sqlite3");
    let existing_manifest = read_manifest(&directory.path)?;
    {
        let c = readonly(&path)?;
        if preflight(identity(&c))? != (0, 1) {
            return Err(StorageError::BadSchema);
        }
        preflight(validate_schema(&c))?;
    }
    let mut c = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.pragma_update(None, "synchronous", "FULL")?;
    let t = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if identity(&t)? != (0, 1) {
        return Err(StorageError::BadSchema);
    }
    validate_schema(&t)?;
    t.execute_batch("CREATE TABLE IF NOT EXISTS rit_storage_identity (k TEXT PRIMARY KEY NOT NULL, v TEXT NOT NULL CHECK(length(v)>0))")?;
    t.pragma_update(None, "application_id", APPLICATION_ID)?;
    let (db_uuid, generation_token) = if let Some(m) = existing_manifest {
        (m.db_uuid, m.generation_token)
    } else {
        (manifest::new_db_uuid(), manifest::random_token_hex()?)
    };
    t.execute(
        "INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('db_uuid',?1)",
        params![db_uuid],
    )?;
    t.execute(
        "INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('generation_token',?1)",
        params![generation_token],
    )?;
    t.commit()?;
    let fid = directory.file_identity("rit.sqlite3")?;
    let (db_uuid, generation_token) = ensure_db_identity(&c)?;
    write_manifest_atomic(
        &directory,
        &manifest::Manifest {
            manifest_version: 1,
            state: manifest::ManifestState::AdoptionReady,
            application_id: APPLICATION_ID,
            user_version: 1,
            db_uuid,
            generation_token,
            volume_serial: None,
            file_id: fid,
        },
    )
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(manifest::MANIFEST_NAME)
}
fn read_manifest(dir: &Path) -> Result<Option<manifest::Manifest>, StorageError> {
    match fs::read(manifest_path(dir)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StorageError::Io(e)),
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|_| StorageError::BadSchema),
    }
}
fn write_manifest_atomic(
    directory: &secure_dir::Guard,
    m: &manifest::Manifest,
) -> Result<(), StorageError> {
    let tmp = directory
        .path
        .join(format!(".{}.tmp", manifest::MANIFEST_NAME));
    let data = serde_json::to_vec_pretty(m).map_err(|_| StorageError::BadSchema)?;
    let mut f = fs::File::create(&tmp)?;
    use std::io::Write;
    f.write_all(&data)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, manifest_path(&directory.path))?;
    #[cfg(target_os = "windows")]
    let _ = directory.flush_dir();
    #[cfg(not(target_os = "windows"))]
    {
        let _ = directory;
    }
    Ok(())
}
fn ensure_db_identity(c: &Connection) -> Result<(String, String), StorageError> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS rit_storage_identity (k TEXT PRIMARY KEY NOT NULL, v TEXT NOT NULL CHECK(length(v)>0))")?;
    let mut stmt = c.prepare("SELECT k,v FROM rit_storage_identity")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let uuid = rows
        .get("db_uuid")
        .cloned()
        .ok_or(StorageError::BadSchema)?;
    let tok = rows
        .get("generation_token")
        .cloned()
        .ok_or(StorageError::BadSchema)?;
    if tok.is_empty() {
        return Err(StorageError::BadSchema);
    }
    Ok((uuid, tok))
}
fn verify_manifest_binding(
    directory: &secure_dir::Guard,
    c: &Connection,
) -> Result<(), StorageError> {
    let Some(m) = read_manifest(&directory.path)? else {
        return Err(StorageError::BadSchema);
    };
    if !matches!(
        m.state,
        manifest::ManifestState::Ready | manifest::ManifestState::AdoptionReady
    ) {
        return Err(StorageError::BadSchema);
    }
    if m.application_id != APPLICATION_ID || m.user_version != 1 {
        return Err(StorageError::BadSchema);
    }
    let (db_uuid, tok) = ensure_db_identity(c)?;
    if db_uuid != m.db_uuid || tok != m.generation_token {
        return Err(StorageError::BadSchema);
    }
    if let (Some(expected), Some(actual)) =
        (m.file_id.as_ref(), directory.file_identity("rit.sqlite3")?)
        && expected != &actual
    {
        return Err(StorageError::BadSchema);
    }
    Ok(())
}

pub struct Registry {
    connection: Connection,
    _directory: secure_dir::Guard,
}
impl Registry {
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        let directory = secure_dir::prepare(dir, true)?
            .ok_or(StorageError::UnsafeDirectory("missing data directory"))?;
        let _lock = directory.lock_initialization()?;
        let created = directory.create_database_if_missing()?;
        let path = directory.path.join("rit.sqlite3");
        let existing_manifest = read_manifest(&directory.path)?;
        let mut needs_writable_recovery = false;
        if !created {
            let existing = readonly(&path)?;
            let classified = match preflight(validate_marked(&existing)) {
                Ok(()) => Ok(()),
                Err(StorageError::RecoveryBlocked) => {
                    needs_writable_recovery = true;
                    Ok(())
                }
                Err(e) => Err(e),
            };
            if existing_manifest.is_none() {
                let _ = directory.file_identity("rit.sqlite3")?;
                return Err(classified.err().unwrap_or(StorageError::BadSchema));
            }
            classified?;
        } else {
            let intent = manifest::Manifest {
                manifest_version: 1,
                state: manifest::ManifestState::InitInProgress,
                application_id: APPLICATION_ID,
                user_version: 1,
                db_uuid: manifest::new_db_uuid(),
                generation_token: manifest::random_token_hex()?,
                volume_serial: None,
                file_id: None,
            };
            write_manifest_atomic(&directory, &intent)?;
        }
        let mut c = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        c.pragma_update(None, "synchronous", "FULL")?;
        if needs_writable_recovery {
            let _: i64 = c
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                .map_err(StorageError::Sql)?;
        }
        let t = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if created {
            if identity(&t)? != (0, 0) {
                return Err(StorageError::BadSchema);
            }
            t.execute_batch(&format!("{REPOSITORIES_SQL}; {LOCATIONS_SQL};"))?;
            t.execute_batch(manifest::IDENTITY_TABLE_SQL)?;
            t.pragma_update(None, "application_id", APPLICATION_ID)?;
            t.pragma_update(None, "user_version", 1)?;
            if let Some(m) = existing_manifest
                .as_ref()
                .filter(|m| m.state == manifest::ManifestState::InitInProgress)
            {
                t.execute(
                    "INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('db_uuid',?1)",
                    params![m.db_uuid],
                )?;
                t.execute("INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('generation_token',?1)", params![m.generation_token])?;
            } else {
                t.execute(
                    "INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('db_uuid',?1)",
                    params![manifest::new_db_uuid()],
                )?;
                t.execute("INSERT OR REPLACE INTO rit_storage_identity(k,v) VALUES('generation_token',?1)", params![manifest::random_token_hex()?])?;
            }
        } else {
            validate_marked(&t)?;
        }
        t.commit()?;
        if created {
            let fid = directory.file_identity("rit.sqlite3")?;
            let (db_uuid, generation_token) = ensure_db_identity(&c)?;
            let m = manifest::Manifest {
                manifest_version: 1,
                state: manifest::ManifestState::Ready,
                application_id: APPLICATION_ID,
                user_version: 1,
                db_uuid,
                generation_token,
                volume_serial: None,
                file_id: fid,
            };
            write_manifest_atomic(&directory, &m)?;
        } else {
            verify_manifest_binding(&directory, &c)?;
            if let Ok((db_uuid, generation_token)) = ensure_db_identity(&c) {
                let fid = directory.file_identity("rit.sqlite3")?;
                let _ = write_manifest_atomic(
                    &directory,
                    &manifest::Manifest {
                        manifest_version: 1,
                        state: manifest::ManifestState::Ready,
                        application_id: APPLICATION_ID,
                        user_version: 1,
                        db_uuid,
                        generation_token,
                        volume_serial: None,
                        file_id: fid,
                    },
                );
            }
        }
        Ok(Self {
            connection: c,
            _directory: directory,
        })
    }
    pub fn open_readonly(dir: &Path) -> Result<Option<Self>, StorageError> {
        match diagnose(dir)? {
            None => return Ok(None),
            Some(1) => {}
            Some(_) => return Err(StorageError::SchemaNeedsMigration),
        }
        let directory = secure_dir::prepare(dir, false)?
            .ok_or(StorageError::UnsafeDirectory("data directory disappeared"))?;
        let c = Connection::open_with_flags(
            directory.path.join("rit.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        preflight(validate_marked(&c))?;
        Ok(Some(Self {
            connection: c,
            _directory: directory,
        }))
    }
    pub fn register(
        &mut self,
        inspection: &Inspection,
        name: Option<&str>,
    ) -> Result<(Repository, bool), StorageError> {
        if name.is_some_and(|n| n.trim().is_empty()) {
            return Err(StorageError::InvalidName);
        }
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| StorageError::Clock)?
                .as_secs(),
        )
        .map_err(|_| StorageError::Clock)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM repositories WHERE common_git_dir=?1",
                [&inspection.common_git_dir],
                |r| r.get(0),
            )
            .optional()?;
        let created = existing.is_none();
        let id = match existing {
            Some(s) => s.parse::<RepositoryId>().map_err(|_| StorageError::BadId)?,
            None => RepositoryId::new(),
        };
        let id_str = id.to_string();
        if created {
            let default_name = Path::new(&inspection.location)
                .file_name()
                .and_then(|v| v.to_str())
                .filter(|v| !v.is_empty())
                .unwrap_or("repository");
            tx.execute("INSERT INTO repositories(id,common_git_dir,display_name,object_format,is_bare,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",params![id_str,inspection.common_git_dir,name.unwrap_or(default_name),inspection.object_format,inspection.bare,now])?;
        } else {
            tx.execute("UPDATE repositories SET display_name=COALESCE(?1,display_name),object_format=?2,updated_at=?3 WHERE id=?4",params![name,inspection.object_format,now,id_str])?;
        }
        let owner: Option<String> = tx
            .query_row(
                "SELECT repository_id FROM repository_locations WHERE path=?1",
                [&inspection.location],
                |r| r.get(0),
            )
            .optional()?;
        if owner.as_deref().is_some_and(|owner| owner != id_str) {
            return Err(StorageError::LocationConflict);
        }
        let kind = if inspection.bare { "bare" } else { "worktree" };
        tx.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(path) DO UPDATE SET git_dir=excluded.git_dir,kind=excluded.kind,last_seen_at=excluded.last_seen_at",params![id_str,inspection.location,inspection.git_dir,kind,now])?;
        tx.commit()?;
        let repository = self.get(id)?.ok_or(StorageError::BadId)?;
        Ok((repository, created))
    }
    pub fn get(&self, id: RepositoryId) -> Result<Option<Repository>, StorageError> {
        let row=self.connection.query_row("SELECT id,display_name,common_git_dir,object_format,is_bare,created_at,updated_at FROM repositories WHERE id=?1",[id.to_string()], |r| Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
        let Some((s, display_name, common_git_dir, object_format, is_bare, created_at, updated_at)) =
            row
        else {
            return Ok(None);
        };
        let id = s.parse().map_err(|_| StorageError::BadId)?;
        let mut stmt=self.connection.prepare("SELECT path,git_dir,kind,first_seen_at,last_seen_at FROM repository_locations WHERE repository_id=?1 ORDER BY id")?;
        let locations = stmt
            .query_map([s], |r| {
                Ok(Location {
                    path: r.get(0)?,
                    git_dir: r.get(1)?,
                    kind: r.get(2)?,
                    first_seen_at: r.get(3)?,
                    last_seen_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Repository {
            id,
            display_name,
            common_git_dir,
            object_format,
            is_bare,
            created_at,
            updated_at,
            locations,
        }))
    }
    pub fn list(&self) -> Result<Vec<Repository>, StorageError> {
        let mut stmt = self
            .connection
            .prepare("SELECT id FROM repositories ORDER BY created_at,id")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|s| {
                self.get(s.parse().map_err(|_| StorageError::BadId)?)?
                    .ok_or(StorageError::BadId)
            })
            .collect()
    }
    pub fn quick_check(&self) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))?
            == "ok")
    }
}
pub fn diagnose(dir: &Path) -> Result<Option<i64>, StorageError> {
    let Some(directory) = secure_dir::prepare(dir, false)? else {
        return Ok(None);
    };
    let path = directory.path.join("rit.sqlite3");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StorageError::Io(e)),
        Ok(_) => {}
    }
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    c.busy_timeout(Duration::from_secs(5))?;
    preflight(validate_marked(&c))?;
    Ok(Some(1))
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    #[test]
    fn actual_registry_connection_enforces_foreign_keys() {
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let registry = Registry::open(tmp.path()).unwrap();
        let enabled: i64 = registry
            .connection
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(enabled, 1);
        let result=registry.connection.execute("INSERT INTO repository_locations(repository_id,path,git_dir,kind,first_seen_at,last_seen_at) VALUES('missing','p','g','worktree',0,0)",[]);
        assert!(
            matches!(result,Err(rusqlite::Error::SqliteFailure(ref e,_)) if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY)
        );
    }
}
