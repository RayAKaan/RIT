//! SQLite-backed local registry of repositories, not source history.
#![forbid(unsafe_code)]
use rit_core::repository::{Inspection, Location, Repository, RepositoryId};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

/// Registry failures never trigger deletion or recreation of existing data.
#[derive(Debug, Error)]
pub enum StorageError {
    /// File or directory creation failure.
    #[error("cannot prepare data directory: {0}")]
    Io(#[from] std::io::Error),
    /// SQLite error, including corruption, locked and integrity errors.
    #[error("SQLite registry error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// Database schema is newer than this executable supports.
    #[error("unsupported registry schema version {0}; upgrade RIT")]
    FutureSchema(i64),
    /// The local system clock is not valid.
    #[error("system clock is before Unix epoch or out of range")]
    Clock,
    /// A stored ID is malformed.
    #[error("invalid repository identifier stored in registry")]
    BadId,
    /// The given path is not safely representable.
    #[error("data path is not valid Unicode")]
    NonUnicode,
    /// Data directories require an explicit or standard base path.
    #[error("no platform data directory is available; supply --data-dir")]
    NoDataDir,
    /// An observed location is already associated with another repository.
    #[error(
        "repository location belongs to another registered repository; relocation requires manual review"
    )]
    LocationConflict,
    /// An existing registry needs a migration before it can be read.
    #[error("registry schema requires migration; run a registration after backing up")]
    SchemaNeedsMigration,
    /// Display names are required and cannot be whitespace only.
    #[error("repository display name must not be empty")]
    InvalidName,
}
/// Resolve explicit, environment, then platform-standard data location; no cwd fallback.
pub fn data_dir(explicit: Option<&Path>) -> Result<PathBuf, StorageError> {
    if let Some(p) = explicit {
        return Ok(p.to_owned());
    }
    if let Some(p) = std::env::var_os("RIT_DATA_DIR")
        && !p.is_empty()
    {
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
/// Local SQLite repository registry.
pub struct Registry {
    connection: Connection,
}
impl Registry {
    /// Create/open the database and transactionally migrate schema 0 to schema 1.
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        fs::create_dir_all(dir)?;
        let mut c = Connection::open(dir.join("rit.sqlite3"))?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        c.pragma_update(None, "synchronous", "FULL")?;
        let t = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = t.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            return Err(StorageError::FutureSchema(version));
        }
        if version == 0 {
            t.execute_batch("CREATE TABLE repositories (id TEXT PRIMARY KEY NOT NULL, common_git_dir TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL CHECK(length(trim(display_name))>0), object_format TEXT NOT NULL, is_bare INTEGER NOT NULL CHECK(is_bare IN (0,1)), created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL); CREATE TABLE repository_locations (id INTEGER PRIMARY KEY, repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE, path TEXT NOT NULL UNIQUE, git_dir TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('worktree','bare')), first_seen_at INTEGER NOT NULL, last_seen_at INTEGER NOT NULL); PRAGMA user_version=1;")?;
        }
        t.commit()?;
        Ok(Self { connection: c })
    }
    /// Open an existing schema-1 registry without creating or migrating any files.
    pub fn open_readonly(dir: &Path) -> Result<Option<Self>, StorageError> {
        match diagnose(dir)? {
            None => return Ok(None),
            Some(1) => {}
            Some(_) => return Err(StorageError::SchemaNeedsMigration),
        }
        let c = Connection::open_with_flags(
            dir.join("rit.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Some(Self { connection: c }))
    }
    /// Register or update a location atomically, preserving repository identity and existing name unless explicitly supplied.
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
    /// Look up a single registered ID.
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
    /// List persisted records without scanning Git worktrees.
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
    /// Run SQLite's quick integrity check.
    pub fn quick_check(&self) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))?
            == "ok")
    }
}
/// Inspect an existing database without creating or migrating it.
/// Returns `None` when it has not been initialized.
pub fn diagnose(dir: &Path) -> Result<Option<i64>, StorageError> {
    let path = dir.join("rit.sqlite3");
    match fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StorageError::Io(e)),
        Ok(_) => {}
    }
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.busy_timeout(Duration::from_secs(5))?;
    let v: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if v > 1 {
        return Err(StorageError::FutureSchema(v));
    }
    let ok: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(StorageError::Sql(rusqlite::Error::InvalidQuery));
    }
    Ok(Some(v))
}
