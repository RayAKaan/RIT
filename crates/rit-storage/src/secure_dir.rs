//! Experimental local storage boundary. Unsupported platforms fail closed.
#[cfg(any(target_os="linux",target_os="windows",all(not(target_os="linux"),not(target_os="windows"))))]
#[allow(unused_imports)]
use crate::StorageError;
#[cfg(any(target_os="linux",target_os="windows",all(not(target_os="linux"),not(target_os="windows"))))]
#[allow(unused_imports)]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "windows", all(not(target_os = "linux"), not(target_os = "windows"))))]
#[cfg(any(target_os = "linux", target_os = "windows", all(not(target_os = "linux"), not(target_os = "windows"))))]
#[cfg(any(target_os="linux", target_os="windows", all(not(target_os="linux"), not(target_os="windows"))))]

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use rustix::fs::{AtFlags, Mode, OFlags, fstat, mkdirat, open, openat, statat};
    use rustix::io::Errno;
    use std::os::fd::OwnedFd;
    use std::path::{Component, PathBuf};

    /// Holds the validated directory open until the SQLite connection is closed.
    pub struct Guard {
        _dir: OwnedFd,
        /// Absolute path traversed without following symlinks.
        pub path: PathBuf,
    }
    impl Guard {
        pub fn lock_initialization(&self) -> Result<OwnedFd, StorageError> {
            use rustix::fs::{FlockOperation, flock};
            let fd = openat(
                &self._dir,
                ".rit-storage.lock",
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(std::io::Error::from)?;
            let st = fstat(&fd).map_err(std::io::Error::from)?;
            if st.st_mode & 0o170000 != 0o100000
                || st.st_uid != rustix::process::geteuid().as_raw()
                || st.st_nlink != 1
                || st.st_mode & 0o077 != 0
            {
                return Err(err(
                    "initialization lock must be a private regular single-link file",
                ));
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                match flock(&fd, FlockOperation::NonBlockingLockExclusive) {
                    Ok(()) => return Ok(fd),
                    Err(Errno::WOULDBLOCK) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(Errno::WOULDBLOCK) => return Err(err("initialization lock timed out")),
                    Err(e) => return Err(std::io::Error::from(e).into()),
                }
            }
        }
        pub fn create_database_if_missing(&self) -> Result<bool, StorageError> {
            match openat(
                &self._dir,
                "rit.sqlite3",
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            ) {
                Ok(fd) => {
                    drop(fd);
                    Ok(true)
                }
                Err(Errno::EXIST) => Ok(false),
                Err(e) => Err(std::io::Error::from(e).into()),
            }
        }

        pub fn file_identity(&self, name: &str) -> Result<Option<String>, StorageError> {
            match statat(&self._dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(Errno::NOENT) => Ok(None),
                Err(e) => Err(std::io::Error::from(e).into()),
                Ok(s) => {
                    if s.st_mode & 0o170000 != 0o100000 { return Err(err("storage object is not a regular file")); }
                    if s.st_uid != rustix::process::geteuid().as_raw() { return Err(err("storage object not owned by effective user")); }
                    Ok(Some(format!("dev:{:x}:ino:{:x}:nlink:{}", s.st_dev, s.st_ino, s.st_nlink)))
                }
            }
        }
    }
    fn err(reason: &'static str) -> StorageError {
        StorageError::UnsafeDirectory(reason)
    }
    fn check_ancestor(fd: &OwnedFd, uid: u32) -> Result<(), StorageError> {
        let s = fstat(fd).map_err(std::io::Error::from)?;
        if s.st_uid != 0 && s.st_uid != uid {
            return Err(err("ancestor is owned by another user"));
        }
        // A sticky directory prevents nonowners from renaming/removing an existing
        // child owned by a different principal, if the sticky directory owner is trusted.
        if s.st_mode & 0o022 != 0 && s.st_mode & 0o1000 == 0 {
            return Err(err(
                "ancestor writable by another user without sticky protection",
            ));
        }
        Ok(())
    }
    fn check_final(fd: &OwnedFd, uid: u32) -> Result<(), StorageError> {
        let s = fstat(fd).map_err(std::io::Error::from)?;
        if s.st_uid != uid || s.st_mode & 0o077 != 0 {
            return Err(err(
                "data directory must be owned by the effective user and owner-only (0700)",
            ));
        }
        Ok(())
    }
    fn check_files(fd: &OwnedFd, uid: u32) -> Result<(), StorageError> {
        for name in [
            "rit.sqlite3",
            "rit.sqlite3-journal",
            "rit.sqlite3-wal",
            "rit.sqlite3-shm",
            ".rit-storage.lock",
        ] {
            match statat(fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(Errno::NOENT) => (),
                Err(e) => return Err(std::io::Error::from(e).into()),
                Ok(s) => {
                    if s.st_mode & 0o170000 != 0o100000 {
                        return Err(err("database or sidecar is not a regular file"));
                    }
                    if s.st_uid != uid {
                        return Err(err(
                            "database or sidecar is not owned by the effective user",
                        ));
                    }
                    if s.st_nlink != 1 {
                        return Err(StorageError::HardLinkedDatabase);
                    }
                    if s.st_mode & 0o077 != 0 {
                        return Err(err(
                            "database or sidecar must be owner-only (0600); back it up and fix permissions before retrying",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn prepare(input: &Path, create: bool) -> Result<Option<Guard>, StorageError> {
        if input
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(err("parent traversal is not allowed"));
        }
        let path = if input.is_absolute() {
            input.to_owned()
        } else {
            std::env::current_dir()?.join(input)
        };
        let parts = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Err(err("filesystem root cannot be the data directory"));
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut dir = open("/", flags, Mode::empty()).map_err(std::io::Error::from)?;
        let uid = rustix::process::geteuid().as_raw();
        check_ancestor(&dir, uid)?;
        for (i, name) in parts.iter().enumerate() {
            let next = match openat(&dir, name, flags, Mode::empty()) {
                Ok(fd) => fd,
                Err(Errno::NOENT) if !create => return Ok(None),
                Err(Errno::NOENT) => {
                    match mkdirat(&dir, name, Mode::from_raw_mode(0o700)) {
                        Ok(()) | Err(Errno::EXIST) => (),
                        Err(e) => return Err(std::io::Error::from(e).into()),
                    }
                    openat(&dir, name, flags, Mode::empty()).map_err(std::io::Error::from)?
                }
                Err(e) => return Err(std::io::Error::from(e).into()),
            };
            if i + 1 == parts.len() {
                check_final(&next, uid)?;
            } else {
                check_ancestor(&next, uid)?;
            }
            dir = next;
        }
        check_files(&dir, uid)?;
        Ok(Some(Guard { _dir: dir, path }))
    }
}
#[cfg(target_os = "linux")]
pub use linux::{Guard, prepare};

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub struct Guard { pub path: std::path::PathBuf }
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub struct LockGuard;
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
impl Guard {
    pub fn lock_initialization(&self) -> Result<LockGuard, StorageError> { Err(StorageError::UnsafeDirectory("unsupported platform")) }
    pub fn create_database_if_missing(&self) -> Result<bool, StorageError> { Err(StorageError::UnsafeDirectory("unsupported platform")) }
    pub fn file_identity(&self, _name: &str) -> Result<Option<String>, StorageError> { Err(StorageError::UnsafeDirectory("unsupported platform")) }
}
#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn prepare(_input: &Path, _create: bool) -> Result<Option<Guard>, StorageError> {
    Err(StorageError::UnsafeDirectory("secure storage is not implemented on this platform; no data was opened"))
}
// Windows native backend (experimental, Phase 2.1 remediation).
#[cfg(target_os = "windows")]
mod windows {
    pub use crate::secure_dir_windows::{Guard, prepare};
}
#[cfg(target_os = "windows")]
pub use windows::{Guard, prepare};