use command_group::{CommandGroup, GroupChild};
use rit_core::repository::{Head, Inspection};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

/// Failures from Git discovery or a read-only Git operation. Raw stderr is intentionally hidden.
#[derive(Debug, Error)]
pub enum GitError {
    /// The operation is not available on this platform or before validation.
    #[error("operation unavailable: {0}")]
    Unavailable(&'static str),
    /// Creation was interrupted after a durable provisioning manifest was reserved.
    #[error("operation {id} provisioning interrupted; reconcile before retrying")]
    Provisioning { id: String },
    /// Git executable was unavailable or could not be spawned.
    #[error("cannot start Git executable: {0}")]
    Launch(#[source] io::Error),
    /// Git version could not be parsed.
    #[error("unrecognized Git version output")]
    Version,
    /// A selected checkpoint path changed or became unreadable during recheck.
    #[error("checkpoint path changed or became unreadable during capture: {path}")]
    CheckpointChanged {
        /// Workspace-relative path.
        path: String,
    },
    /// Older Git is unsupported.
    #[error("Git {0} is unsupported; Git 2.46 or newer and required capabilities are needed")]
    Unsupported(String),
    /// The command ran longer than the deadline.
    #[error("Git operation timed out")]
    Timeout,
    /// Bounded output was exceeded.
    #[error("Git output exceeded the configured limit")]
    OutputLimit,
    /// The process group could not be terminated.
    #[error("could not terminate the Git process group: {0}")]
    Termination(#[source] io::Error),
    /// An operation returned an error. Raw Git output is not displayed.
    #[error(
        "Git inspection failed for {operation} (exit {code:?}); check repository access and Git configuration"
    )]
    Exit {
        /// Operation name.
        operation: &'static str,
        /// Exit code if present.
        code: Option<i32>,
    },
    /// Git output did not match the expected machine-readable format.
    #[error("unsupported or malformed Git output: {0}")]
    Parse(&'static str),
    /// Input path could not be accessed or canonicalized.
    #[error("cannot access repository path {path:?}: {source}")]
    Path {
        /// The supplied path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// A path cannot be safely represented in the Phase 2 model.
    #[error("repository or status path is not valid Unicode; this path is not supported")]
    NonUnicode,
}

type RestoreFiles = Vec<(String, String, bool, Vec<u8>)>;
type PreparedRestore = (rit_core::checkpoint::RestorePlan, Vec<u8>, RestoreFiles);
type PreparedExtraction = (rit_core::checkpoint::ExtractionPlan, RestoreFiles);

/// An executable plus limits for typed Git operations.
#[derive(Debug, Clone)]
pub struct Git {
    executable: OsString,
    version: String,
    timeout: Duration,
    limit: usize,
}
impl Git {
    /// Discover Git using the explicit override, then `RIT_GIT_EXECUTABLE`, then PATH.
    pub fn discover(explicit: Option<&OsStr>) -> Result<Self, GitError> {
        let selected = explicit
            .map(OsString::from)
            .or_else(|| std::env::var_os("RIT_GIT_EXECUTABLE"))
            .unwrap_or_else(|| OsString::from("git"));
        let selected_path = PathBuf::from(&selected);
        let executable = if selected_path.components().count() > 1 || selected_path.is_absolute() {
            selected_path
                .canonicalize()
                .map_err(GitError::Launch)?
                .into_os_string()
        } else {
            let mut names = vec![selected_path.clone()];
            if cfg!(windows) && selected_path.extension().is_none() {
                let mut n = selected_path.as_os_str().to_owned();
                n.push(".exe");
                names.insert(0, PathBuf::from(n));
            }
            let paths = std::env::var_os("PATH").ok_or_else(|| {
                GitError::Launch(io::Error::new(io::ErrorKind::NotFound, "PATH is not set"))
            })?;
            let found = std::env::split_paths(&paths)
                .filter(|p| p.is_absolute())
                .flat_map(|p| names.iter().map(move |n| p.join(n)))
                .find(|p| p.is_file())
                .ok_or_else(|| {
                    GitError::Launch(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Git executable not found on PATH",
                    ))
                })?;
            found
                .canonicalize()
                .map_err(GitError::Launch)?
                .into_os_string()
        };
        let mut git = Self {
            executable,
            version: String::new(),
            timeout: Duration::from_secs(20),
            limit: 8 * 1024 * 1024,
        };
        let out = git.run(None, &[OsString::from("--version")], "version")?;
        let text = String::from_utf8(out).map_err(|_| GitError::Version)?;
        let version = text
            .trim()
            .strip_prefix("git version ")
            .ok_or(GitError::Version)?
            .to_string();
        let mut parts = version.split('.');
        let major = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or(GitError::Version)?;
        let minor = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .ok_or(GitError::Version)?;
        if (major, minor) < (2, 46) {
            return Err(GitError::Unsupported(version));
        }
        git.version = version;
        Ok(git)
    }
    /// Executable path or name supplied for OS lookup.
    pub fn executable(&self) -> &OsStr {
        &self.executable
    }
    /// Detected version text, including any platform suffix.
    pub fn version(&self) -> &str {
        &self.version
    }
    /// Set per-operation resource limits.
    pub fn with_limits(mut self, timeout: Duration, bytes: usize) -> Self {
        self.timeout = timeout;
        self.limit = bytes;
        self
    }
    fn run(
        &self,
        cwd: Option<&Path>,
        args: &[OsString],
        operation: &'static str,
    ) -> Result<Vec<u8>, GitError> {
        self.run_internal(cwd, args, operation, &[], false, None)
            .map(|v| v.0)
    }
    fn run_internal(
        &self,
        cwd: Option<&Path>,
        args: &[OsString],
        operation: &'static str,
        extra_env: &[(&str, OsString)],
        allow_exit_one: bool,
        input: Option<&[u8]>,
    ) -> Result<(Vec<u8>, i32), GitError> {
        // Status and worktree diffs may invoke clean filters, even with --no-ext-diff.
        // Fail closed unless the Linux x86-64 child-process restriction can be installed.
        let restricted = matches!(operation, "status" | "diff" | "conflict merge")
            || operation.starts_with("workspace ");
        let _filter = if restricted {
            Some(no_child_filter()?)
        } else {
            None
        };
        let mut cmd = if let Some(filter) = &_filter {
            let mut c = Command::new("/usr/bin/setpriv");
            c.args(["--no-new-privs", "--seccomp-filter"])
                .arg(filter.path())
                .arg("--")
                .arg(&self.executable);
            c
        } else {
            Command::new(&self.executable)
        };
        // All GIT_* variables can alter repository discovery/config/commands. Preserve normal OS environment.
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("LC_ALL", "C")
            .args([
                "--no-optional-locks",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.untrackedCache=false",
            ])
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        if let Some(path) = cwd {
            cmd.current_dir(path);
        }
        let mut child = cmd.group_spawn().map_err(GitError::Launch)?;
        let writer = if let Some(input) = input {
            if input.len() > 8_388_608 {
                terminate(child)?;
                return Err(GitError::OutputLimit);
            }
            let mut stdin = child
                .inner()
                .stdin
                .take()
                .ok_or(GitError::Parse("stdin pipe"))?;
            let bytes = input.to_vec();
            Some(thread::spawn(move || {
                use std::io::Write;
                stdin.write_all(&bytes)
            }))
        } else {
            None
        };
        let out = child
            .inner()
            .stdout
            .take()
            .ok_or(GitError::Parse("stdout pipe"))?;
        let err = child
            .inner()
            .stderr
            .take()
            .ok_or(GitError::Parse("stderr pipe"))?;
        let exceeded = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        for (index, mut reader) in [
            (0, Box::new(out) as Box<dyn Read + Send>),
            (1, Box::new(err) as Box<dyn Read + Send>),
        ] {
            let tx = tx.clone();
            let flag = exceeded.clone();
            let limit = self.limit;
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut buf = [0u8; 8192];
                let result = loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break Ok(bytes),
                        Ok(n) if bytes.len().saturating_add(n) <= limit => {
                            bytes.extend_from_slice(&buf[..n])
                        }
                        Ok(_) => {
                            flag.store(true, Ordering::Relaxed);
                            break Err(());
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break Err(()),
                    }
                };
                let _ = tx.send((index, result));
            });
        }
        drop(tx);
        let deadline = Instant::now() + self.timeout;
        let mut status = None;
        let mut outputs = [None, None];
        loop {
            while let Ok((i, result)) = rx.try_recv() {
                outputs[i] = Some(result);
            }
            if exceeded.load(Ordering::Relaxed) {
                terminate(child)?;
                return Err(GitError::OutputLimit);
            }
            if status.is_none() {
                status = child.try_wait().map_err(GitError::Launch)?;
            }
            if status.is_some() && outputs.iter().all(Option::is_some) {
                break;
            }
            if Instant::now() >= deadline {
                terminate(child)?;
                return Err(GitError::Timeout);
            }
            thread::sleep(Duration::from_millis(5));
        }
        if let Some(writer) = writer {
            writer
                .join()
                .map_err(|_| GitError::Parse("stdin writer panicked"))?
                .map_err(GitError::Launch)?;
        }
        let status = status.ok_or(GitError::Parse("process status"))?;
        let code = status.code().unwrap_or(-1);
        if !status.success() && !(allow_exit_one && code == 1) {
            return Err(GitError::Exit {
                operation,
                code: status.code(),
            });
        }
        let stderr = outputs[1]
            .take()
            .ok_or(GitError::Parse("stderr"))?
            .map_err(|_| GitError::Parse("stderr read"))?;
        if operation == "conflict merge" && !stderr.is_empty() {
            return Err(GitError::Unavailable(
                "merge simulation produced diagnostics, potentially from an unsupported driver",
            ));
        }
        let out = outputs[0]
            .take()
            .ok_or(GitError::Parse("stdout"))?
            .map_err(|_| GitError::Parse("stdout read"))?;
        Ok((out, code))
    }
    fn query(
        &self,
        cwd: &Path,
        args: &[&str],
        operation: &'static str,
    ) -> Result<String, GitError> {
        let args: Vec<_> = args.iter().map(OsString::from).collect();
        let bytes = self.run(Some(cwd), &args, operation)?;
        let mut text = String::from_utf8(bytes).map_err(|_| GitError::NonUnicode)?;
        if text.is_empty() {
            return Ok(text);
        }
        if !text.ends_with('\n') {
            return Err(GitError::Parse(operation));
        }
        text.pop();
        if cfg!(windows) && text.ends_with('\r') {
            text.pop();
        }
        Ok(text)
    }
    /// Read local branches, tags, and remote-tracking refs without opening a RIT registry.
    pub fn references(
        &self,
        input: &Path,
    ) -> Result<Vec<rit_core::git_object::GitReference>, GitError> {
        use rit_core::git_object::{GitReference, ObjectId};
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        if !cwd.is_dir() {
            return Err(GitError::Parse("repository path must be a directory"));
        }
        let format = self.object_format(&cwd)?;
        let text = self.query(
            &cwd,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)%00%(*objectname)",
                "refs/heads/",
                "refs/tags/",
                "refs/remotes/",
            ],
            "references",
        )?;
        let mut result = Vec::new();
        for line in text.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            if fields.len() != 4
                || fields[0].is_empty()
                || !matches!(fields[2], "commit" | "tree" | "blob" | "tag")
            {
                return Err(GitError::Parse("reference fields"));
            }
            let object = ObjectId::new(format, fields[1]).map_err(GitError::Parse)?;
            let peeled = if fields[3].is_empty() {
                None
            } else {
                Some(ObjectId::new(format, fields[3]).map_err(GitError::Parse)?)
            };
            result.push(GitReference {
                name: fields[0].to_owned(),
                object,
                object_type: fields[2].to_owned(),
                peeled,
            });
        }
        Ok(result)
    }
    /// Inspect up to `limit` commits reachable from HEAD, without messages or file contents.
    /// This is a snapshot: shallow repositories and concurrent ref movement affect results.
    pub fn history(
        &self,
        input: &Path,
        limit: u32,
    ) -> Result<Vec<rit_core::git_object::GitCommit>, GitError> {
        use rit_core::git_object::{GitCommit, ObjectId};
        if !(1..=1000).contains(&limit) {
            return Err(GitError::Parse("history limit must be between 1 and 1000"));
        }
        let ins = self.inspect(input)?;
        if matches!(ins.head, Head::Unborn { .. }) {
            return Ok(Vec::new());
        }
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let format = self.object_format(&cwd)?;
        let max = format!("--max-count={limit}");
        let text = self.query(
            &cwd,
            &[
                "log",
                "--no-show-signature",
                "--no-notes",
                "--no-decorate",
                "--format=%H%x00%P%x00%ct",
                &max,
                "HEAD",
                "--",
            ],
            "history",
        )?;
        let mut result = Vec::new();
        for line in text.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            if fields.len() != 3 {
                return Err(GitError::Parse("history fields"));
            }
            let id = ObjectId::new(format, fields[0]).map_err(GitError::Parse)?;
            let parents = fields[1]
                .split_whitespace()
                .map(|s| ObjectId::new(format, s).map_err(GitError::Parse))
                .collect::<Result<Vec<_>, _>>()?;
            let committed_at = fields[2]
                .parse()
                .map_err(|_| GitError::Parse("commit timestamp"))?;
            result.push(GitCommit {
                id,
                parents,
                committed_at,
            });
        }
        Ok(result)
    }
    fn object_format(&self, cwd: &Path) -> Result<rit_core::git_object::ObjectFormat, GitError> {
        let value = self.query(cwd, &["rev-parse", "--show-object-format"], "object format")?;
        rit_core::git_object::ObjectFormat::parse(&value)
            .ok_or(GitError::Parse("unsupported object format"))
    }
    /// Inspect Git worktree inventory with NUL-delimited porcelain output.
    pub fn worktrees(
        &self,
        input: &Path,
    ) -> Result<Vec<rit_core::git_object::WorktreeRecord>, GitError> {
        use rit_core::git_object::{ObjectId, WorktreeRecord};
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let format = self.object_format(&cwd)?;
        let args = ["worktree", "list", "--porcelain", "-z"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let bytes = self.run(Some(&cwd), &args, "worktree inventory")?;
        if !bytes.ends_with(&[0, 0]) {
            return Err(GitError::Parse("worktree terminator"));
        }
        let mut records = Vec::new();
        for record in bytes
            .split(|b| *b == 0)
            .collect::<Vec<_>>()
            .split(|part| part.is_empty())
        {
            if record.is_empty() {
                continue;
            }
            let mut path = None;
            let mut head = None;
            let mut branch = None;
            let mut detached = false;
            let mut bare = false;
            let mut locked = false;
            let mut prunable = false;
            for raw in record {
                let item = std::str::from_utf8(raw).map_err(|_| GitError::NonUnicode)?;
                if let Some(value) = item.strip_prefix("worktree ") {
                    if path.replace(value.to_owned()).is_some() {
                        return Err(GitError::Parse("duplicate worktree path"));
                    }
                } else if let Some(value) = item.strip_prefix("HEAD ") {
                    if value.bytes().all(|b| b == b'0') {
                        head = None
                    } else {
                        head = Some(ObjectId::new(format, value).map_err(GitError::Parse)?);
                    }
                } else if let Some(value) = item.strip_prefix("branch ") {
                    branch = Some(value.to_owned())
                } else if item == "detached" {
                    detached = true
                } else if item == "bare" {
                    bare = true
                } else if item == "locked" || item.starts_with("locked ") {
                    locked = true
                } else if item == "prunable" || item.starts_with("prunable ") {
                    prunable = true
                } else {
                    return Err(GitError::Parse("unknown worktree record"));
                }
            }
            records.push(WorktreeRecord {
                path: path
                    .filter(|x| !x.is_empty())
                    .ok_or(GitError::Parse("missing worktree path"))?,
                head,
                branch,
                detached,
                bare,
                locked,
                prunable,
            });
        }
        Ok(records)
    }
    /// Inspect staged and unstaged tracked file differences without invoking diff drivers.
    pub fn diff(&self, input: &Path) -> Result<rit_core::git_object::DiffObservation, GitError> {
        use rit_core::git_object::{DiffObservation, FileDifference};
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let ins = self.inspect(&cwd)?;
        if ins.bare {
            return Err(GitError::Parse("bare repository has no working tree"));
        }
        let query = |staged: bool| -> Result<Vec<FileDifference>, GitError> {
            let mut args = vec![
                OsString::from("diff"),
                OsString::from("--no-ext-diff"),
                OsString::from("--no-textconv"),
                OsString::from("--no-renames"),
                OsString::from("--name-status"),
                OsString::from("-z"),
            ];
            if staged {
                args.push(OsString::from("--cached"));
            }
            args.push(OsString::from("--"));
            let bytes = self.run(Some(&cwd), &args, "diff")?;
            if bytes.is_empty() {
                return Ok(Vec::new());
            }
            if !bytes.ends_with(&[0]) {
                return Err(GitError::Parse("diff terminator"));
            }
            let mut fields = bytes.split(|b| *b == 0);
            let mut out = Vec::new();
            while let Some(kind) = fields.next() {
                if kind.is_empty() {
                    break;
                }
                let path = fields
                    .next()
                    .filter(|v| !v.is_empty())
                    .ok_or(GitError::Parse("diff path"))?;
                let status = std::str::from_utf8(kind).map_err(|_| GitError::NonUnicode)?;
                if status.len() != 1 || !"ACDMRTUXB".contains(status) {
                    return Err(GitError::Parse("diff status"));
                }
                out.push(FileDifference {
                    status: status.into(),
                    path: std::str::from_utf8(path)
                        .map_err(|_| GitError::NonUnicode)?
                        .into(),
                });
            }
            Ok(out)
        };
        Ok(DiffObservation {
            staged: query(true)?,
            unstaged: query(false)?,
        })
    }
    /// Read bounded Python, TypeScript and Go regular blobs from the current immutable HEAD commit.
    /// Does not inspect uncommitted changes, follow worktree symlinks, or execute repository scripts.
    pub fn source_snapshot(
        &self,
        input: &Path,
    ) -> Result<rit_core::git_object::SourceSnapshot, GitError> {
        self.source_snapshot_revision(input, None)
    }
    /// Read a bounded immutable snapshot from an explicitly selected commit or branch.
    pub fn source_snapshot_revision(
        &self,
        input: &Path,
        revision: Option<&str>,
    ) -> Result<rit_core::git_object::SourceSnapshot, GitError> {
        use rit_core::git_object::{ObjectId, SourceFile, SourceSnapshot};
        let ins = self.inspect(input)?;
        let commit = if let Some(revision) = revision {
            if revision.is_empty() || revision.len() > 256 || revision.contains('\0') {
                return Err(GitError::Parse("invalid source revision"));
            }
            self.query(
                input,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{revision}^{{commit}}"),
                ],
                "source revision",
            )?
        } else {
            match ins.head {
                Head::Branch { commit, .. } | Head::Detached { commit } => commit,
                Head::Unborn { .. } => {
                    return Err(GitError::Parse("unborn repository has no source snapshot"));
                }
            }
        };
        let format = rit_core::git_object::ObjectFormat::parse(&ins.object_format)
            .ok_or(GitError::Parse("unsupported object format"))?;
        let commit = ObjectId::new(format, &commit).map_err(GitError::Parse)?;
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let args = ["ls-tree", "--full-tree", "-r", "-z", commit.as_hex()]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let tree = self.run(Some(&cwd), &args, "source tree")?;
        if !tree.is_empty() && !tree.ends_with(&[0]) {
            return Err(GitError::Parse("tree terminator"));
        }
        let mut selections = Vec::new();
        let mut omitted = 0usize;
        for entry in tree.split(|b| *b == 0) {
            if entry.is_empty() {
                continue;
            }
            let tab = entry
                .iter()
                .position(|b| *b == b'\t')
                .ok_or(GitError::Parse("tree fields"))?;
            let meta = std::str::from_utf8(&entry[..tab]).map_err(|_| GitError::NonUnicode)?;
            let path = std::str::from_utf8(&entry[tab + 1..]).map_err(|_| GitError::NonUnicode)?;
            if !(path.ends_with(".py")
                || path.ends_with(".ts")
                || path.ends_with(".go")
                || path == "go.mod"
                || path.ends_with("/go.mod"))
            {
                continue;
            }
            let mut fields = meta.split(' ');
            let mode = fields.next().ok_or(GitError::Parse("tree mode"))?;
            let kind = fields.next().ok_or(GitError::Parse("tree kind"))?;
            let oid = fields.next().ok_or(GitError::Parse("tree oid"))?;
            if fields.next().is_some() {
                return Err(GitError::Parse("tree metadata"));
            }
            if kind != "blob" || !matches!(mode, "100644" | "100755") {
                omitted += 1;
                continue;
            }
            let oid = ObjectId::new(format, oid).map_err(GitError::Parse)?;
            selections.push((path.to_owned(), oid));
            if selections.len() > 500 {
                return Err(GitError::Parse("source file count exceeds 500"));
            }
        }
        let mut files = Vec::new();
        let mut total = 0usize;
        for (path, id) in selections {
            let size = self
                .query(&cwd, &["cat-file", "-s", id.as_hex()], "blob size")?
                .parse::<usize>()
                .map_err(|_| GitError::Parse("blob size"))?;
            if size > 1_048_576 {
                return Err(GitError::Parse("source file exceeds 1 MiB"));
            }
            total = total.checked_add(size).ok_or(GitError::OutputLimit)?;
            if total > 8_388_608 {
                return Err(GitError::Parse("source snapshot exceeds 8 MiB"));
            }
            let args = [
                OsString::from("cat-file"),
                OsString::from("blob"),
                OsString::from(id.as_hex()),
            ];
            let bytes = self.run(Some(&cwd), &args, "source blob")?;
            if bytes.len() != size {
                return Err(GitError::Parse("blob size changed"));
            }
            files.push(SourceFile { path, bytes });
        }
        Ok(SourceSnapshot {
            snapshot_id: format!("commit:{}", commit.as_hex()),
            non_atomic: false,
            scope: "selected committed snapshot; uncommitted worktree content is not analyzed",
            commit,
            files,
            omitted_non_regular: omitted,
        })
    }
    /// Observe tracked and untracked source bytes without following symlinks or running filters.
    /// The observation is non-atomic, omits ignored paths and may become stale immediately.
    pub fn source_snapshot_worktree(
        &self,
        input: &Path,
    ) -> Result<rit_core::git_object::SourceSnapshot, GitError> {
        use rit_core::git_object::{ObjectFormat, ObjectId, SourceFile, SourceSnapshot};
        use sha2::{Digest, Sha256};
        let ins = self.inspect(input)?;
        let root = ins
            .worktree
            .as_deref()
            .ok_or(GitError::Unavailable("bare repository has no worktree"))?;
        let head = match ins.head {
            Head::Branch { commit, .. } | Head::Detached { commit } => commit,
            Head::Unborn { .. } => {
                return Err(GitError::Parse("unborn worktree has no snapshot base"));
            }
        };
        let commit = ObjectId::new(
            ObjectFormat::parse(&ins.object_format).ok_or(GitError::Parse("object format"))?,
            &head,
        )
        .map_err(GitError::Parse)?;
        let output = self.run(
            Some(Path::new(root)),
            &[
                "ls-files".into(),
                "--cached".into(),
                "--others".into(),
                "--exclude-standard".into(),
                "-z".into(),
            ],
            "workspace source list",
        )?;
        if !output.is_empty() && !output.ends_with(&[0]) {
            return Err(GitError::Parse("source list terminator"));
        }
        let mut paths = std::collections::BTreeSet::new();
        for raw in output.split(|b| *b == 0).filter(|v| !v.is_empty()) {
            let path = std::str::from_utf8(raw).map_err(|_| GitError::NonUnicode)?;
            if !(path.ends_with(".py")
                || path.ends_with(".ts")
                || path.ends_with(".go")
                || path == "go.mod"
                || path.ends_with("/go.mod"))
            {
                continue;
            }
            if Path::new(path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(GitError::Parse("worktree source path"));
            }
            paths.insert(path.to_owned());
            if paths.len() > 500 {
                return Err(GitError::Parse("source file count exceeds 500"));
            }
        }
        let mut files = Vec::new();
        let mut omitted = 0;
        let mut omitted_paths = Vec::new();
        let mut total = 0usize;
        for path in paths {
            match read_worktree_file(Path::new(root), &path)? {
                Some(bytes) => {
                    total = total
                        .checked_add(bytes.len())
                        .ok_or(GitError::OutputLimit)?;
                    if total > 8_388_608 {
                        return Err(GitError::Parse("worktree snapshot exceeds 8 MiB"));
                    }
                    files.push(SourceFile { path, bytes });
                }
                None => {
                    omitted += 1;
                    omitted_paths.push(path);
                }
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"rit-worktree-source-v1\0");
        hash.update(commit.as_hex().as_bytes());
        for path in omitted_paths {
            hash.update(b"omitted\0");
            hash.update((path.len() as u64).to_be_bytes());
            hash.update(path.as_bytes());
        }
        for f in &files {
            hash.update((f.path.len() as u64).to_be_bytes());
            hash.update(f.path.as_bytes());
            hash.update((f.bytes.len() as u64).to_be_bytes());
            hash.update(&f.bytes)
        }
        let snapshot_id = format!("worktree-sha256:{:x}", hash.finalize());
        Ok(SourceSnapshot {
            commit,
            files,
            omitted_non_regular: omitted,
            snapshot_id,
            non_atomic: true,
            scope: "non-atomic worktree observation of tracked and untracked supported files; ignored files excluded; staged bytes are not separately analyzed",
        })
    }
    /// Observe staged source blobs from the real Git index, separately from unstaged worktree bytes.
    /// The index and HEAD are rechecked, but no atomicity or authenticated provenance is claimed.
    pub fn source_snapshot_index(
        &self,
        input: &Path,
    ) -> Result<rit_core::git_object::SourceSnapshot, GitError> {
        use rit_core::git_object::{ObjectFormat, ObjectId, SourceFile, SourceSnapshot};
        use sha2::{Digest, Sha256};
        let ins = self.inspect(input)?;
        let root = ins
            .worktree
            .as_deref()
            .ok_or(GitError::Unavailable("bare repository has no index"))?;
        let head = match ins.head {
            Head::Branch { commit, .. } | Head::Detached { commit } => commit,
            Head::Unborn { .. } => {
                return Err(GitError::Unavailable("unborn index analysis not supported"));
            }
        };
        let format =
            ObjectFormat::parse(&ins.object_format).ok_or(GitError::Parse("object format"))?;
        let commit = ObjectId::new(format, &head).map_err(GitError::Parse)?;
        let args = ["ls-files", "-z", "--stage"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let raw = self.run(
            Some(Path::new(root)),
            &args,
            "workspace index analysis list",
        )?;
        if !raw.is_empty() && !raw.ends_with(&[0]) {
            return Err(GitError::Parse("index analysis terminator"));
        }
        let mut selected = Vec::new();
        let mut omitted = 0usize;
        for row in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = row
                .iter()
                .position(|b| *b == b'\t')
                .ok_or(GitError::Parse("index analysis row"))?;
            let path = std::str::from_utf8(&row[tab + 1..]).map_err(|_| GitError::NonUnicode)?;
            if !(path.ends_with(".py")
                || path.ends_with(".ts")
                || path.ends_with(".go")
                || path == "go.mod"
                || path.ends_with("/go.mod"))
            {
                continue;
            }
            validate_checkpoint_path(path)?;
            let meta = std::str::from_utf8(&row[..tab]).map_err(|_| GitError::NonUnicode)?;
            let f = meta.split(' ').collect::<Vec<_>>();
            if f.len() != 3 {
                return Err(GitError::Parse("index analysis metadata"));
            }
            if f[2] != "0" {
                return Err(GitError::Unavailable(
                    "unmerged source index entries cannot form a single staged graph",
                ));
            }
            if !matches!(f[0], "100644" | "100755") {
                omitted += 1;
                continue;
            }
            selected.push((
                path.to_owned(),
                ObjectId::new(format, f[1]).map_err(GitError::Parse)?,
            ));
            if selected.len() > 500 {
                return Err(GitError::Unavailable(
                    "staged source file count exceeds 500",
                ));
            }
        }
        let mut files = Vec::new();
        let mut total = 0usize;
        for (path, oid) in selected {
            let bytes = self.bounded_blob(Path::new(root), &oid, 1_048_576)?;
            total = total
                .checked_add(bytes.len())
                .ok_or(GitError::OutputLimit)?;
            if total > 8_388_608 {
                return Err(GitError::Unavailable("staged source exceeds 8 MiB"));
            }
            files.push(SourceFile { path, bytes });
        }
        let after = self.run(
            Some(Path::new(root)),
            &args,
            "workspace index analysis recheck",
        )?;
        if after != raw {
            return Err(GitError::Unavailable(
                "index changed during staged analysis",
            ));
        }
        let after_head = self.query(
            Path::new(root),
            &["rev-parse", "--verify", "HEAD"],
            "workspace index analysis HEAD",
        )?;
        if after_head != head {
            return Err(GitError::Unavailable("HEAD changed during staged analysis"));
        }
        let mut digest = Sha256::new();
        digest.update(b"rit-index-source-v1\0");
        digest.update(head.as_bytes());
        digest.update(&raw);
        Ok(SourceSnapshot {
            commit,
            files,
            omitted_non_regular: omitted,
            snapshot_id: format!("index-sha256:{:x}", digest.finalize()),
            non_atomic: true,
            scope: "rechecked Git index stage-0 source blobs; excludes unstaged, untracked and ignored bytes; can change or be reverted between observations",
        })
    }
    /// Simulate an isolated Git merge in a temporary object directory; no refs or worktrees are changed.
    /// Custom driver configuration is refused. The temporary merge tree is not retained.
    pub fn simulate_merge(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
    ) -> Result<rit_core::conflict::MergeSimulation, GitError> {
        self.simulate_merge_mode(repo, left, right, false)
    }
    fn simulate_merge_mode(
        &self,
        repo: &Path,
        left: &str,
        right: &str,
        persist_tree: bool,
    ) -> Result<rit_core::conflict::MergeSimulation, GitError> {
        use rit_core::{
            conflict::{MergeMessage, MergeSimulation, MergeStage},
            git_object::ObjectId,
        };
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "merge simulation not validated on this platform",
            ));
        }
        let ins = self.inspect(repo)?;
        let cwd = repo.canonicalize().map_err(|source| GitError::Path {
            path: repo.into(),
            source,
        })?;
        let format = self.object_format(&cwd)?;
        let resolve = |v: &str| -> Result<ObjectId, GitError> {
            if v.is_empty() || v.len() > 256 || v.contains('\0') {
                return Err(GitError::Parse("invalid merge revision"));
            }
            let oid = self.query(
                &cwd,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{v}^{{commit}}"),
                ],
                "merge revision",
            )?;
            ObjectId::new(format, &oid).map_err(GitError::Parse)
        };
        let left = resolve(left)?;
        let right = resolve(right)?;
        let bases = self
            .query(
                &cwd,
                &["merge-base", "--all", left.as_hex(), right.as_hex()],
                "merge base",
            )?
            .lines()
            .map(|s| ObjectId::new(format, s).map_err(GitError::Parse))
            .collect::<Result<Vec<_>, _>>()?;
        if bases.is_empty() {
            return Err(GitError::Unavailable("no merge base"));
        }
        let comparison_base = bases[0].clone();
        let changed = |side: &ObjectId| -> Result<Vec<String>, GitError> {
            let args = [
                "diff-tree",
                "-r",
                "--name-only",
                "-z",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                comparison_base.as_hex(),
                side.as_hex(),
                "--",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            let bytes = self.run(Some(&cwd), &args, "conflict merge")?;
            if !bytes.is_empty() && !bytes.ends_with(&[0]) {
                return Err(GitError::Parse("changed paths terminator"));
            }
            bytes
                .split(|b| *b == 0)
                .filter(|b| !b.is_empty())
                .map(|b| {
                    std::str::from_utf8(b)
                        .map(str::to_owned)
                        .map_err(|_| GitError::NonUnicode)
                })
                .collect()
        };
        let changed_left = changed(&left)?;
        let changed_right = changed(&right)?;
        let configured = self.run(
            Some(&cwd),
            &[
                "config".into(),
                "--null".into(),
                "--get-regexp".into(),
                r"^merge\..*\.driver$".into(),
            ],
            "merge driver config",
        );
        match configured {
            Ok(data) if !data.is_empty() => {
                return Err(GitError::Unavailable("custom merge driver configured"));
            }
            Ok(_) | Err(GitError::Exit { code: Some(1), .. }) => (),
            Err(e) => return Err(e),
        }
        let tmp = if persist_tree {
            None
        } else {
            Some(tempfile::tempdir().map_err(GitError::Launch)?)
        };
        let mut env = Vec::new();
        if let Some(tmp) = &tmp {
            let object_dir = Path::new(&ins.common_git_dir)
                .join("objects")
                .canonicalize()
                .map_err(GitError::Launch)?;
            let object_text = object_dir.to_str().ok_or(GitError::NonUnicode)?;
            if object_text.contains('\n') || object_text.contains('\r') {
                return Err(GitError::Unavailable("unsupported object directory path"));
            }
            let objects = tmp.path().join("objects");
            std::fs::create_dir_all(objects.join("info")).map_err(GitError::Launch)?;
            std::fs::create_dir(objects.join("pack")).map_err(GitError::Launch)?;
            std::fs::write(objects.join("info/alternates"), format!("{object_text}\n"))
                .map_err(GitError::Launch)?;
            env.push(("GIT_OBJECT_DIRECTORY", objects.into_os_string()));
        }
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "merge-tree",
            "--write-tree",
            "--messages",
            "-z",
            left.as_hex(),
            right.as_hex(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        let (out, code) =
            self.run_internal(Some(&cwd), &args, "conflict merge", &env, true, None)?;
        if !out.ends_with(&[0]) {
            return Err(GitError::Parse("merge output terminator"));
        }
        let mut parts = out.split(|b| *b == 0);
        let tree = std::str::from_utf8(parts.next().ok_or(GitError::Parse("merge tree"))?)
            .map_err(|_| GitError::NonUnicode)?;
        let ephemeral_tree = ObjectId::new(format, tree).map_err(GitError::Parse)?;
        let mut stages = Vec::new();
        loop {
            let record = parts
                .next()
                .ok_or(GitError::Parse("merge stage terminator"))?;
            if record.is_empty() {
                break;
            }
            let tab = record
                .iter()
                .position(|b| *b == b'\t')
                .ok_or(GitError::Parse("merge stage path"))?;
            let meta = std::str::from_utf8(&record[..tab]).map_err(|_| GitError::NonUnicode)?;
            let mut f = meta.split(' ');
            let mode = f.next().ok_or(GitError::Parse("merge mode"))?;
            let object = f.next().ok_or(GitError::Parse("merge object"))?;
            let stage = f
                .next()
                .ok_or(GitError::Parse("merge stage"))?
                .parse::<u8>()
                .map_err(|_| GitError::Parse("merge stage"))?;
            if f.next().is_some()
                || !(1..=3).contains(&stage)
                || !matches!(mode, "100644" | "100755" | "120000" | "160000")
            {
                return Err(GitError::Parse("invalid merge stage"));
            }
            stages.push(MergeStage {
                path: std::str::from_utf8(&record[tab + 1..])
                    .map_err(|_| GitError::NonUnicode)?
                    .into(),
                stage,
                mode: mode.into(),
                object: ObjectId::new(format, object).map_err(GitError::Parse)?,
            });
            if stages.len() > 50_000 {
                return Err(GitError::OutputLimit);
            }
        }
        let mut messages = Vec::new();
        while let Some(count) = parts.next() {
            if count.is_empty() {
                break;
            }
            let count = std::str::from_utf8(count)
                .map_err(|_| GitError::NonUnicode)?
                .parse::<usize>()
                .map_err(|_| GitError::Parse("merge message count"))?;
            if count > 1000 {
                return Err(GitError::OutputLimit);
            }
            let mut paths = Vec::new();
            for _ in 0..count {
                paths.push(
                    std::str::from_utf8(parts.next().ok_or(GitError::Parse("merge message path"))?)
                        .map_err(|_| GitError::NonUnicode)?
                        .into(),
                );
            }
            let kind =
                std::str::from_utf8(parts.next().ok_or(GitError::Parse("merge message kind"))?)
                    .map_err(|_| GitError::NonUnicode)?
                    .into();
            let detail = std::str::from_utf8(
                parts
                    .next()
                    .ok_or(GitError::Parse("merge message detail"))?,
            )
            .map_err(|_| GitError::NonUnicode)?
            .into();
            messages.push(MergeMessage {
                paths,
                kind,
                detail,
            });
            if messages.len() > 50_000 {
                return Err(GitError::OutputLimit);
            }
        }
        Ok(MergeSimulation {
            left,
            right,
            merge_bases: bases,
            comparison_base,
            changed_left,
            changed_right,
            ephemeral_tree,
            has_conflicts: code == 1,
            stages,
            messages,
            human_review_required: true,
        })
    }
    /// Retrieve a bounded immutable blob for structural evidence without running filters.
    pub fn conflict_blob(
        &self,
        repo: &Path,
        id: &rit_core::git_object::ObjectId,
    ) -> Result<Vec<u8>, GitError> {
        self.bounded_blob(repo, id, 1_048_576)
    }
    fn bounded_blob(
        &self,
        repo: &Path,
        id: &rit_core::git_object::ObjectId,
        limit: usize,
    ) -> Result<Vec<u8>, GitError> {
        let format = self.object_format(repo)?;
        if id.algorithm() != format {
            return Err(GitError::Parse("blob object format"));
        }
        if self.query(repo, &["cat-file", "-t", id.as_hex()], "blob type")? != "blob" {
            return Err(GitError::Parse("not a blob"));
        }
        let size = self
            .query(repo, &["cat-file", "-s", id.as_hex()], "blob size")?
            .parse::<usize>()
            .map_err(|_| GitError::Parse("blob size"))?;
        if size > limit {
            return Err(GitError::Unavailable("blob exceeds configured read limit"));
        }
        let b = self.run(
            Some(repo),
            &["cat-file".into(), "blob".into(), id.as_hex().into()],
            "blob content",
        )?;
        if b.len() != size {
            return Err(GitError::Parse("blob length"));
        }
        Ok(b)
    }
    /// Create a bounded Git-native workspace, never invoking checkout filters or hooks.
    /// Linux x86-64 only; unsupported content is left in a no-checkout worktree.
    pub fn workspace_create(
        &self,
        repo: &Path,
        dest: &Path,
        name: &str,
        base: &str,
    ) -> Result<rit_core::workspace::WorkspaceManifest, GitError> {
        use rit_core::{
            git_object::ObjectId,
            workspace::{WorkspaceId, WorkspaceManifest, WorkspaceOperation},
        };
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "managed workspace writes are not validated on this platform",
            ));
        }
        if name.is_empty() || name.len() > 100 || name.chars().any(|c| c.is_control()) {
            return Err(GitError::Parse("invalid workspace name"));
        }
        if base.is_empty() || base.len() > 256 || base.contains('\0') {
            return Err(GitError::Parse("invalid base reference"));
        }
        let ins = self.inspect(repo)?;
        let root = ins
            .worktree
            .as_deref()
            .ok_or(GitError::Unavailable("bare repository workspace creation"))?;
        let cwd = Path::new(root);
        let format = self.object_format(cwd)?;
        let oid = self.query(
            cwd,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
            "workspace base",
        )?;
        let commit = ObjectId::new(format, &oid).map_err(GitError::Parse)?;
        let target = workspace_target(dest)?;
        let common = Path::new(&ins.common_git_dir);
        let main = Path::new(root);
        if target.starts_with(main)
            || main.starts_with(&target)
            || target.starts_with(common)
            || common.starts_with(&target)
        {
            return Err(GitError::Parse(
                "workspace path overlaps repository or Git directory",
            ));
        }
        let inventory = self.worktrees(cwd)?;
        for w in inventory {
            let p = Path::new(&w.path);
            if target.starts_with(p) || p.starts_with(&target) {
                return Err(GitError::Parse("workspace path overlaps existing worktree"));
            }
        }
        let id = WorkspaceId::new();
        let branch = format!("refs/heads/rit/workspaces/{id}");
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| GitError::Parse("system clock before Unix epoch"))?
            .as_secs();
        let mut manifest = WorkspaceManifest {
            schema_version: 1,
            id,
            name: name.into(),
            path: target.to_str().ok_or(GitError::NonUnicode)?.into(),
            branch: branch.clone(),
            base: commit.clone(),
            created_at,
            state: "provisioning".into(),
            limitation: None,
        };
        let zero = "0".repeat(commit.as_hex().len());
        let op_id = WorkspaceId::new();
        let op_ref = format!("refs/rit/operations/{op_id}");
        let mut op=WorkspaceOperation{schema_version:1,id:op_id,workspace:id,kind:"workspace_create".into(),path:manifest.path.clone(),branch:branch.clone(),base:commit.clone(),state:"created".into(),failure:None,action:"inspect actual Git worktree and branch before any repair; never delete uncertain data".into()};
        let mut op_oid = self.write_json_ref(cwd, format, &op_ref, &op, &zero)?;
        let action = (|| -> Result<(), GitError> {
            let old = self.workspace_write_manifest(cwd, &manifest, &zero)?;
            op.state = "manifest_reserved".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            let args = [
                "-c",
                "core.hooksPath=/dev/null",
                "update-ref",
                branch.as_str(),
                commit.as_hex(),
                zero.as_str(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            self.run(Some(cwd), &args, "workspace branch")?;
            op.state = "branch_reserved".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            // Exclusive private directory; Git accepts an existing empty directory.
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&target)
                    .map_err(|source| GitError::Path {
                        path: target.clone(),
                        source,
                    })?;
            }
            #[cfg(not(unix))]
            {
                return Err(GitError::Unavailable(
                    "workspace path validation unavailable",
                ));
            }
            op.state = "path_reserved".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            let args = vec![
                "-c".into(),
                "core.hooksPath=/dev/null".into(),
                "worktree".into(),
                "add".into(),
                "--no-checkout".into(),
                target.as_os_str().into(),
                branch
                    .strip_prefix("refs/heads/")
                    .ok_or(GitError::Parse("branch ref"))?
                    .into(),
            ];
            self.run(Some(cwd), &args, "workspace worktree add")?;
            op.state = "worktree_created".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            let actual = self
                .worktrees(cwd)?
                .into_iter()
                .find(|w| w.path == manifest.path)
                .ok_or(GitError::Parse("created worktree missing"))?;
            if actual.branch.as_deref() != Some(branch.as_str())
                || actual.head.as_ref() != Some(&commit)
            {
                return Err(GitError::Parse("created worktree mismatch"));
            }
            op.state = "worktree_verified".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            match self.workspace_materialize(&target, &commit) {
                Ok(()) => {
                    manifest.state = "ready".into();
                }
                Err(GitError::Unavailable(reason)) => {
                    manifest.state = "no_checkout".into();
                    manifest.limitation = Some(reason.into());
                }
                Err(e) => return Err(e),
            }
            self.workspace_write_manifest(cwd, &manifest, old.as_hex())?;
            op.state = "completed".into();
            op.action = "none; reconcile before further lifecycle operations".into();
            op_oid = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex())?;
            Ok(())
        })();
        if let Err(error) = action {
            op.failure = Some(format!("provider failed after {}: {}", op.state, error));
            op.action =
                "inspect Git inventory and filesystem; do not delete or automatically replay"
                    .into();
            let _recorded = self.write_json_ref(cwd, format, &op_ref, &op, op_oid.as_hex());
            return Err(GitError::Provisioning { id: id.to_string() });
        }
        Ok(manifest)
    }
    fn workspace_write_manifest(
        &self,
        cwd: &Path,
        m: &rit_core::workspace::WorkspaceManifest,
        old: &str,
    ) -> Result<rit_core::git_object::ObjectId, GitError> {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().map_err(GitError::Launch)?;
        serde_json::to_writer(&mut f, m).map_err(|_| GitError::Parse("manifest serialization"))?;
        f.flush().map_err(GitError::Launch)?;
        let args = vec![
            "hash-object".into(),
            "-w".into(),
            "--no-filters".into(),
            f.path().as_os_str().into(),
        ];
        let out = self.run(Some(cwd), &args, "workspace manifest object")?;
        let oid = std::str::from_utf8(&out)
            .map_err(|_| GitError::NonUnicode)?
            .trim_end();
        let oid = rit_core::git_object::ObjectId::new(m.base.algorithm(), oid)
            .map_err(GitError::Parse)?;
        let reference = format!("refs/rit/workspaces/{}/manifest", m.id);
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            reference.as_str(),
            oid.as_hex(),
            old,
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(cwd), &args, "workspace manifest ref")?;
        Ok(oid)
    }
    fn workspace_materialize(
        &self,
        worktree: &Path,
        commit: &rit_core::git_object::ObjectId,
    ) -> Result<(), GitError> {
        // Read only Git objects, never checkout/smudge. Reject transformed or special files.
        let args = ["ls-tree", "-r", "-z", "--full-tree", commit.as_hex()]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let entries = self.run(Some(worktree), &args, "workspace tree")?;
        let mut files: Vec<(String, String, bool, Vec<u8>)> = Vec::new();
        let mut total = 0usize;
        for entry in entries.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            let tab = entry
                .iter()
                .position(|b| *b == b'\t')
                .ok_or(GitError::Parse("tree entry"))?;
            let meta = std::str::from_utf8(&entry[..tab]).map_err(|_| GitError::NonUnicode)?;
            let mut m = meta.split(' ');
            let mode = m.next().unwrap_or("");
            let kind = m.next().unwrap_or("");
            let oid = m.next().unwrap_or("");
            if m.next().is_some() || kind != "blob" || !matches!(mode, "100644" | "100755") {
                return Err(GitError::Unavailable(
                    "symlinks, submodules and non-regular tree entries require no-checkout",
                ));
            }
            let object = rit_core::git_object::ObjectId::new(commit.algorithm(), oid)
                .map_err(GitError::Parse)?;
            let path = std::str::from_utf8(&entry[tab + 1..])
                .map_err(|_| GitError::NonUnicode)?
                .to_owned();
            if path.len() > 4096
                || Path::new(&path).components().any(|c| match c {
                    std::path::Component::Normal(x) => {
                        x.to_string_lossy().eq_ignore_ascii_case(".git")
                    }
                    _ => true,
                })
            {
                return Err(GitError::Unavailable("unsupported tree path"));
            }
            if files.len() >= 500 {
                return Err(GitError::Unavailable(
                    "more than 500 files require no-checkout",
                ));
            }
            let size = self
                .query(
                    worktree,
                    &["cat-file", "-s", object.as_hex()],
                    "workspace blob size",
                )?
                .parse::<usize>()
                .map_err(|_| GitError::Parse("blob size"))?;
            total = total.checked_add(size).ok_or(GitError::OutputLimit)?;
            if size > 1024 * 1024 || total > 8 * 1024 * 1024 {
                return Err(GitError::Unavailable(
                    "tree exceeds materialization size limits",
                ));
            }
            let data = self.run(
                Some(worktree),
                &["cat-file".into(), "blob".into(), object.as_hex().into()],
                "workspace blob",
            )?;
            if data.len() != size {
                return Err(GitError::Parse("blob size mismatch"));
            }
            files.push((path, oid.into(), mode == "100755", data));
        }
        // Reject configured filters even when unused; do not run them. Query effective
        // repository config and attributes, including info/attributes.
        let config = self.run(
            Some(worktree),
            &[
                "config".into(),
                "--null".into(),
                "--get-regexp".into(),
                r"^filter\..*\.(clean|smudge|process)$".into(),
            ],
            "workspace filter config",
        );
        match config {
            Ok(data) if !data.is_empty() => {
                return Err(GitError::Unavailable(
                    "external filter configuration requires no-checkout",
                ));
            }
            Ok(_) | Err(GitError::Exit { code: Some(1), .. }) => (),
            Err(e) => return Err(e),
        }
        let autocrlf = self.query(
            worktree,
            &["config", "--get", "core.autocrlf"],
            "workspace autocrlf",
        );
        match autocrlf {
            Ok(v) if v != "false" => {
                return Err(GitError::Unavailable("core.autocrlf requires no-checkout"));
            }
            Ok(_) | Err(GitError::Exit { code: Some(1), .. }) => (),
            Err(e) => return Err(e),
        }
        if !files.is_empty() {
            let mut args = vec![
                OsString::from("check-attr"),
                "-z".into(),
                "--source".into(),
                commit.as_hex().into(),
                "filter".into(),
                "working-tree-encoding".into(),
                "ident".into(),
                "text".into(),
                "eol".into(),
                "--".into(),
            ];
            if files.iter().map(|x| x.0.len()).sum::<usize>() > 64 * 1024 {
                return Err(GitError::Unavailable("tree path list exceeds limit"));
            }
            args.extend(files.iter().map(|x| OsString::from(&x.0)));
            let attrs = self.run(Some(worktree), &args, "workspace attributes")?;
            let parts = attrs.split(|b| *b == 0).collect::<Vec<_>>();
            if parts.len() != files.len() * 5 * 3 + 1 {
                return Err(GitError::Parse("attribute response"));
            }
            for triple in parts[..parts.len() - 1].chunks(3) {
                if triple[2] != b"unspecified" && triple[2] != b"unset" {
                    return Err(GitError::Unavailable(
                        "checkout-affecting attributes require no-checkout",
                    ));
                }
            }
        }
        self.run(
            Some(worktree),
            &["read-tree".into(), commit.as_hex().into()],
            "workspace index",
        )?;
        materialize_files(worktree, files)?;
        Ok(())
    }
    /// Reconcile manifest claims with real Git branch and worktree inventory.
    pub fn workspace_reconcile(
        &self,
        repo: &Path,
    ) -> Result<rit_core::workspace::WorkspaceReconciliation, GitError> {
        use rit_core::{
            git_object::ObjectId,
            workspace::{ManagedWorkspaceObservation, WorkspaceManifest, WorkspaceReconciliation},
        };
        let cwd = repo.canonicalize().map_err(|source| GitError::Path {
            path: repo.into(),
            source,
        })?;
        let format = self.object_format(&cwd)?;
        let common = self.inspect(&cwd)?.common_git_dir;
        let worktrees = self.worktrees(&cwd)?;
        let text = self.query(
            &cwd,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/workspaces/",
            ],
            "workspace manifests",
        )?;
        let mut observations = Vec::new();
        let mut branches = std::collections::BTreeSet::new();
        for line in text.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 || f[2] != "blob" {
                return Err(GitError::Parse("workspace manifest ref"));
            }
            let raw = self.run(
                Some(&cwd),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace read manifest",
            )?;
            if raw.len() > 16 * 1024 {
                return Err(GitError::Parse("manifest too large"));
            }
            let m: WorkspaceManifest = serde_json::from_slice(&raw)
                .map_err(|_| GitError::Parse("invalid workspace manifest"))?;
            if m.schema_version != 1
                || f[0] != format!("refs/rit/workspaces/{}/manifest", m.id)
                || m.branch != format!("refs/heads/rit/workspaces/{}", m.id)
                || m.base.algorithm() != format
                || !matches!(m.state.as_str(), "provisioning" | "ready" | "no_checkout")
                || !Path::new(&m.path).is_absolute()
            {
                return Err(GitError::Parse("invalid workspace manifest claims"));
            }
            branches.insert(m.branch.clone());
            let w = worktrees
                .iter()
                .find(|w| w.branch.as_deref() == Some(&m.branch))
                .cloned();
            let mut observation = match &w {
                None => "missing",
                Some(w) if w.path != m.path => "moved",
                Some(w) if w.prunable => "prunable",
                Some(_) if m.state == "provisioning" => "interrupted",
                Some(_) if m.state == "no_checkout" => "no_checkout",
                Some(_) => "present",
            }
            .to_string();
            let status = if observation == "present" {
                self.inspect(Path::new(&m.path))
                    .ok()
                    .filter(|i| i.common_git_dir == common)
                    .and_then(|i| i.status)
            } else {
                None
            };
            if observation == "present" && status.is_none() {
                observation = "inspection_unavailable".into();
            }
            observations.push(ManagedWorkspaceObservation {
                manifest: m,
                manifest_oid: ObjectId::new(format, f[1]).map_err(GitError::Parse)?,
                worktree: w,
                observation,
                status,
            });
        }
        let refs = self.query(
            &cwd,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/rit/workspaces/",
            ],
            "workspace branches",
        )?;
        let orphan_branches = refs
            .lines()
            .filter(|b| !branches.contains(*b))
            .map(str::to_owned)
            .collect();
        let unclaimed_worktrees = worktrees
            .iter()
            .filter_map(|w| {
                w.branch
                    .as_ref()
                    .filter(|b| {
                        b.starts_with("refs/heads/rit/workspaces/") && !branches.contains(*b)
                    })
                    .map(|_| w.path.clone())
            })
            .collect();
        Ok(WorkspaceReconciliation {
            workspaces: observations,
            orphan_branches,
            unclaimed_worktrees,
        })
    }
    /// Inspect removal-related evidence without deleting files, refs, or metadata.
    pub fn workspace_removal_preview(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
    ) -> Result<rit_core::workspace::WorkspaceRemovalPreview, GitError> {
        let w = self
            .workspace_reconcile(repo)?
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == id)
            .ok_or(GitError::Parse("workspace not found"))?;
        let mut blockers = vec![
            "removal disabled: race-safe preservation and durable cleanup are not verified"
                .to_string(),
        ];
        if w.observation != "present" {
            blockers.push(format!("workspace observation: {}", w.observation));
        }
        match &w.status {
            Some(s) if !s.changes.is_empty() => blockers.push(format!(
                "{} staged, unstaged or untracked status entries",
                s.changes.len()
            )),
            None => blockers.push("workspace status unavailable".into()),
            _ => (),
        }
        if w.worktree.as_ref().is_some_and(|v| v.locked) {
            blockers.push("Git worktree is locked".into());
        }
        let mut ignored_paths = None;
        if w.observation == "present" {
            let args = [
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            match self.run(
                Some(Path::new(&w.manifest.path)),
                &args,
                "workspace ignored inventory",
            ) {
                Ok(out) => {
                    if !out.is_empty() && !out.ends_with(&[0]) {
                        return Err(GitError::Parse("ignored paths terminator"));
                    }
                    let mut paths = Vec::new();
                    for p in out.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                        let name = std::str::from_utf8(p).map_err(|_| GitError::NonUnicode)?;
                        paths.push(name.to_owned());
                        if paths.len() > 5000 {
                            return Err(GitError::OutputLimit);
                        }
                    }
                    if !paths.is_empty() {
                        blockers.push(format!("{} ignored file or directory entries", paths.len()));
                    }
                    ignored_paths = Some(paths);
                }
                Err(e) => blockers.push(format!("ignored inventory unavailable: {e}")),
            }
        }
        let submissions = self
            .submission_list(repo)?
            .into_iter()
            .filter(|s| s.workspace == id && s.lifecycle != "cancelled")
            .map(|s| s.id.to_string())
            .collect::<Vec<_>>();
        if !submissions.is_empty() {
            blockers.push(format!("{} active submissions", submissions.len()));
        }
        let checkpoints = self
            .content_checkpoint_list(repo)?
            .into_iter()
            .filter(|c| c.manifest.workspace == id)
            .map(|c| c.manifest.id.to_string())
            .collect();
        Ok(rit_core::workspace::WorkspaceRemovalPreview{workspace:w,ignored_paths,active_submissions:submissions,checkpoint_ids:checkpoints,blockers,executable:false,limitations:vec!["point-in-time evidence can become stale immediately".into(),"Git refs and manifests do not authenticate ownership or authorize removal".into(),"ignored inventory may collapse directories; uncommitted and nested content are not preserved by a preview".into(),"no paths, worktrees, branches or metadata are removed".into()]})
    }
    /// Inspect durable workspace operation intents against live Git metadata without deleting data.
    pub fn workspace_operations(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::workspace::WorkspaceOperationObservation>, GitError> {
        use rit_core::{
            git_object::ObjectId,
            workspace::{WorkspaceOperation, WorkspaceOperationObservation},
        };
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/operations/",
            ],
            "workspace operation refs",
        )?;
        let state = self.workspace_reconcile(repo)?;
        let branches = self.references(repo)?;
        let trees = self.worktrees(repo)?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 || f[2] != "blob" {
                return Err(GitError::Parse("workspace operation ref"));
            }
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace operation read",
            )?;
            if raw.len() > 16_384 {
                return Err(GitError::OutputLimit);
            }
            let op: WorkspaceOperation = serde_json::from_slice(&raw)
                .map_err(|_| GitError::Parse("workspace operation manifest"))?;
            if op.schema_version != 1
                || op.kind != "workspace_create"
                || f[0] != format!("refs/rit/operations/{}", op.id)
                || op.branch != format!("refs/heads/rit/workspaces/{}", op.workspace)
                || op.base.algorithm() != format
                || !Path::new(&op.path).is_absolute()
                || !matches!(
                    op.state.as_str(),
                    "created"
                        | "manifest_reserved"
                        | "branch_reserved"
                        | "path_reserved"
                        | "worktree_created"
                        | "worktree_verified"
                        | "completed"
                )
            {
                return Err(GitError::Parse("workspace operation claims"));
            }
            let m = state
                .workspaces
                .iter()
                .find(|w| w.manifest.id == op.workspace);
            let matches = m.is_some_and(|w| {
                w.manifest.path == op.path
                    && w.manifest.branch == op.branch
                    && w.manifest.base == op.base
                    && matches!(w.observation.as_str(), "present" | "no_checkout")
            });
            let branch_exists = branches.iter().any(|b| b.name == op.branch);
            let actual = trees
                .iter()
                .any(|w| w.path == op.path || w.branch.as_deref() == Some(&op.branch));
            let observation = if matches {
                if op.state == "completed" {
                    "completed"
                } else {
                    "completion_not_recorded"
                }
            } else if op.state == "completed" {
                "completed_state_diverged"
            } else if m.is_none() && branch_exists {
                "orphan_branch"
            } else if m.is_none() && actual {
                "unclaimed_worktree"
            } else if m.is_none() {
                "manifest_missing"
            } else if !branch_exists {
                "branch_missing"
            } else if !actual {
                "worktree_missing_or_incomplete"
            } else {
                "needs_reconciliation"
            };
            let metadata_repair_possible = matches && op.state != "completed";
            result.push(WorkspaceOperationObservation {
                operation: op,
                object: ObjectId::new(format, f[1]).map_err(GitError::Parse)?,
                observation: observation.into(),
                metadata_repair_possible,
            });
        }
        Ok(result)
    }
    /// CAS-complete only already-matching workspace intents; never changes files or branches.
    pub fn workspace_repair_operations(&self, repo: &Path) -> Result<usize, GitError> {
        let format = self.object_format(repo)?;
        let ops = self.workspace_operations(repo)?;
        let mut repaired = 0;
        for item in ops {
            if item.observation != "completion_not_recorded" || !item.metadata_repair_possible {
                continue;
            }
            let mut op = item.operation;
            op.state = "completed".into();
            op.failure = None;
            op.action =
                "metadata-only reconciliation; no filesystem or branch repair performed".into();
            let reference = format!("refs/rit/operations/{}", op.id);
            self.write_json_ref(repo, format, &reference, &op, item.object.as_hex())?;
            repaired += 1;
        }
        Ok(repaired)
    }
    /// Lock or unlock an identified RIT branch's actual worktree; metadata is not authentication.
    pub fn workspace_lock(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
        reason: Option<&str>,
    ) -> Result<(), GitError> {
        let state = self.workspace_reconcile(repo)?;
        let w = state
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == id)
            .ok_or(GitError::Parse("workspace not found"))?;
        if w.observation != "present" && w.observation != "no_checkout" {
            return Err(GitError::Unavailable(
                "workspace must reconcile to its manifest path",
            ));
        }
        let mut args: Vec<OsString> = ["-c", "core.hooksPath=/dev/null", "worktree"]
            .into_iter()
            .map(OsString::from)
            .collect();
        if let Some(reason) = reason {
            if reason.is_empty() || reason.len() > 256 || reason.chars().any(|c| c.is_control()) {
                return Err(GitError::Parse("invalid lock reason"));
            }
            args.extend(["lock".into(), "--reason".into(), reason.into()]);
        } else {
            args.push("unlock".into());
        }
        args.push(w.manifest.path.into());
        self.run(Some(repo), &args, "workspace lock")?;
        Ok(())
    }
    fn checkpoint_hash_blob(
        &self,
        repo: &Path,
        format: rit_core::git_object::ObjectFormat,
        bytes: &[u8],
    ) -> Result<rit_core::git_object::ObjectId, GitError> {
        let args = ["hash-object", "--no-filters", "-w", "--stdin"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (out, _) = self.run_internal(
            Some(repo),
            &args,
            "workspace checkpoint blob",
            &[],
            false,
            Some(bytes),
        )?;
        rit_core::git_object::ObjectId::new(
            format,
            std::str::from_utf8(&out)
                .map_err(|_| GitError::NonUnicode)?
                .trim_end(),
        )
        .map_err(GitError::Parse)
    }
    fn checkpoint_tree(
        &self,
        repo: &Path,
        format: rit_core::git_object::ObjectFormat,
        entries: &[(String, rit_core::git_object::ObjectId, bool)],
    ) -> Result<rit_core::git_object::ObjectId, GitError> {
        let mut input = Vec::new();
        for (name, id, is_tree) in entries {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
            {
                return Err(GitError::Parse("checkpoint tree name"));
            }
            input.extend_from_slice(
                format!(
                    "{} {} {}\t{}",
                    if *is_tree { "040000" } else { "100644" },
                    if *is_tree { "tree" } else { "blob" },
                    id.as_hex(),
                    name
                )
                .as_bytes(),
            );
            input.push(0);
        }
        let args = ["mktree", "-z"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (out, _) = self.run_internal(
            Some(repo),
            &args,
            "workspace checkpoint tree",
            &[],
            false,
            Some(&input),
        )?;
        rit_core::git_object::ObjectId::new(
            format,
            std::str::from_utf8(&out)
                .map_err(|_| GitError::NonUnicode)?
                .trim_end(),
        )
        .map_err(GitError::Parse)
    }
    /// Capture with conservative default resource budgets.
    pub fn content_checkpoint_create(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
        include_untracked: bool,
        ignored: &[String],
    ) -> Result<rit_core::checkpoint::ContentCheckpointRecord, GitError> {
        self.content_checkpoint_create_with_limits(
            repo,
            id,
            include_untracked,
            ignored,
            &rit_core::checkpoint::CaptureLimits::default(),
        )
    }
    /// Capture bounded raw content with explicitly selected resource budgets; not atomic.
    pub fn content_checkpoint_create_with_limits(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
        include_untracked: bool,
        ignored: &[String],
        limits: &rit_core::checkpoint::CaptureLimits,
    ) -> Result<rit_core::checkpoint::ContentCheckpointRecord, GitError> {
        use rit_core::{
            checkpoint::{
                CapturedIndexEntry, CapturedWorktreeEntry, CheckpointId, ContentCheckpoint,
                ContentCheckpointRecord,
            },
            git_object::ObjectId,
        };
        use sha2::{Digest, Sha256};
        limits.validate().map_err(GitError::Parse)?;
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "content checkpoints unavailable on this platform",
            ));
        }
        if ignored.len() > 100 {
            return Err(GitError::Parse("too many selected ignored paths"));
        }
        let reconciled = self.workspace_reconcile(repo)?;
        let w = reconciled
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == id && w.observation == "present")
            .ok_or(GitError::Unavailable("workspace not present and ready"))?;
        let root = Path::new(&w.manifest.path);
        let ins = self.inspect(root)?;
        let base = match ins.head {
            Head::Branch { commit, .. } | Head::Detached { commit } => commit,
            Head::Unborn { .. } => return Err(GitError::Parse("unborn checkpoint")),
        };
        let format = self.object_format(root)?;
        let base = ObjectId::new(format, &base).map_err(GitError::Parse)?;
        let index_path = Path::new(&ins.git_dir);
        let index_bytes = read_worktree_file(index_path, "index")?.ok_or(GitError::Unavailable(
            "workspace index missing or not regular",
        ))?;
        let index_blob = self.checkpoint_hash_blob(root, format, &index_bytes)?;
        let staged_raw = self.run(
            Some(root),
            &["ls-files".into(), "--stage".into(), "-z".into()],
            "workspace checkpoint index",
        )?;
        let mut staged = Vec::new();
        let mut tracked = std::collections::BTreeSet::new();
        let mut limitations=vec!["Non-atomic multi-file observation; it can become stale immediately".into(),"Ignored content excluded except explicitly selected files".into(),"LFS pointer blobs and observed raw worktree bytes do not establish LFS object availability".into(),"POSIX metadata other than the executable bit, ACLs, xattrs and hardlink identity are not captured".into()];
        for row in staged_raw.split(|b| *b == 0).filter(|b| !b.is_empty()) {
            let tab = row
                .iter()
                .position(|b| *b == b'\t')
                .ok_or(GitError::Parse("index row"))?;
            let meta = std::str::from_utf8(&row[..tab]).map_err(|_| GitError::NonUnicode)?;
            let mut fields = meta.split(' ');
            let mode = fields.next().ok_or(GitError::Parse("index mode"))?;
            let oid = fields.next().ok_or(GitError::Parse("index object"))?;
            let stage = fields
                .next()
                .ok_or(GitError::Parse("index stage"))?
                .parse::<u8>()
                .map_err(|_| GitError::Parse("index stage"))?;
            if fields.next().is_some()
                || stage > 3
                || !matches!(mode, "100644" | "100755" | "120000" | "160000")
            {
                return Err(GitError::Parse("index entry"));
            }
            let path = std::str::from_utf8(&row[tab + 1..])
                .map_err(|_| GitError::NonUnicode)?
                .to_owned();
            validate_checkpoint_path(&path)?;
            tracked.insert(path.clone());
            if mode == "160000" {
                limitations.push(format!("Submodule {path}: gitlink recorded; submodule content and dirty state not captured"));
            }
            if stage != 0 {
                limitations.push(format!(
                    "Unmerged index stages for {path}: captured; Git merge-operation metadata is not captured"
                ));
            }
            staged.push(CapturedIndexEntry {
                path,
                mode: mode.into(),
                stage,
                object: ObjectId::new(format, oid).map_err(GitError::Parse)?,
            });
            if staged.len() > 2000 {
                return Err(GitError::Unavailable(
                    "index entry count exceeds checkpoint limit",
                ));
            }
        }
        let untracked_raw = self.run(
            Some(root),
            &[
                "ls-files".into(),
                "--others".into(),
                "--exclude-standard".into(),
                "-z".into(),
            ],
            "workspace checkpoint untracked",
        )?;
        let untracked = untracked_raw
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                std::str::from_utf8(b)
                    .map(str::to_owned)
                    .map_err(|_| GitError::NonUnicode)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        if !include_untracked && !untracked.is_empty() {
            limitations.push("Untracked files were not selected for capture".into())
        }
        let mut selected: std::collections::BTreeMap<String, String> =
            tracked.into_iter().map(|p| (p, "tracked".into())).collect();
        if include_untracked {
            for path in &untracked {
                validate_checkpoint_path(path)?;
                selected
                    .entry(path.clone())
                    .or_insert_with(|| "untracked".into());
            }
        }
        for path in ignored {
            validate_checkpoint_path(path)?;
            let result = self.run(
                Some(root),
                &[
                    "check-ignore".into(),
                    "--no-index".into(),
                    "-q".into(),
                    "--".into(),
                    path.into(),
                ],
                "workspace checkpoint ignored",
            );
            match result {
                Ok(_) => (),
                Err(GitError::Exit { code: Some(1), .. }) => {
                    return Err(GitError::Parse("selected path is not ignored"));
                }
                Err(e) => return Err(e),
            }
            if selected.contains_key(path) {
                return Err(GitError::Parse("duplicate selected ignored path"));
            }
            selected.insert(path.clone(), "selected_ignored".into());
        }
        if selected.len() > limits.max_paths {
            return Err(GitError::Unavailable(
                "checkpoint file count exceeds selected budget",
            ));
        }
        let mut total = 0usize;
        let mut worktree = Vec::new();
        let mut observed =
            std::collections::BTreeMap::<String, Option<(String, bool, [u8; 32])>>::new();
        for (path, category) in selected {
            if staged.iter().any(|e| e.path == path && e.mode == "160000") {
                continue;
            }
            match read_checkpoint_entry(root, &path, limits.max_file_bytes)? {
                None => {
                    if category != "tracked" {
                        return Err(GitError::Unavailable("selected file disappeared"));
                    }
                    observed.insert(path.clone(), None);
                    worktree.push(CapturedWorktreeEntry {
                        path,
                        category,
                        kind: "deleted".into(),
                        executable: false,
                        blob: None,
                    })
                }
                Some((kind, executable, bytes)) => {
                    total = total
                        .checked_add(bytes.len())
                        .ok_or(GitError::OutputLimit)?;
                    if total > limits.max_total_bytes {
                        return Err(GitError::Unavailable(
                            "checkpoint total exceeds selected budget",
                        ));
                    }
                    let digest: [u8; 32] = Sha256::digest(&bytes).into();
                    observed.insert(path.clone(), Some((kind.into(), executable, digest)));
                    let blob = self.checkpoint_hash_blob(root, format, &bytes)?;
                    worktree.push(CapturedWorktreeEntry {
                        path,
                        category,
                        kind: kind.into(),
                        executable,
                        blob: Some(blob),
                    });
                }
            }
        }
        // Re-read every selected path after capture; this detects many cross-file races,
        // but cannot prove atomicity or detect a change that is reversed between reads.
        for (path, expected) in &observed {
            let now = read_checkpoint_entry(root, path, limits.max_file_bytes)
                .map_err(|_| GitError::CheckpointChanged { path: path.clone() })?
                .map(|(kind, exec, bytes)| {
                    let digest: [u8; 32] = Sha256::digest(&bytes).into();
                    (kind.to_owned(), exec, digest)
                });
            if &now != expected {
                return Err(GitError::CheckpointChanged { path: path.clone() });
            }
        }
        if include_untracked {
            let now = self.run(
                Some(root),
                &[
                    "ls-files".into(),
                    "--others".into(),
                    "--exclude-standard".into(),
                    "-z".into(),
                ],
                "workspace checkpoint untracked recheck",
            )?;
            let now = now
                .split(|b| *b == 0)
                .filter(|b| !b.is_empty())
                .map(|b| {
                    std::str::from_utf8(b)
                        .map(str::to_owned)
                        .map_err(|_| GitError::NonUnicode)
                })
                .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
            if let Some(path) = untracked.symmetric_difference(&now).next() {
                return Err(GitError::CheckpointChanged { path: path.clone() });
            }
        }
        let after_index = read_worktree_file(index_path, "index")?
            .ok_or(GitError::Unavailable("index disappeared"))?;
        if index_bytes != after_index {
            return Err(GitError::Unavailable("index changed during checkpoint"));
        }
        let after_head = self.query(root, &["rev-parse", "--verify", "HEAD"], "checkpoint HEAD")?;
        if after_head != base.as_hex() {
            return Err(GitError::Unavailable("HEAD changed during checkpoint"));
        }
        let shared = self.query(
            root,
            &["rev-parse", "--shared-index-path"],
            "checkpoint shared index",
        )?;
        if !shared.is_empty() {
            limitations
                .push("Split-index shared-index file is not captured as raw index bytes".into())
        }
        let id = CheckpointId::new();
        let manifest=ContentCheckpoint{schema_version:3,id,workspace:w.manifest.id,base:base.clone(),index_blob:index_blob.clone(),staged,worktree,capture_limits:Some(limits.clone()),created_at:Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|GitError::Parse("system clock before Unix epoch"))?.as_secs()),include_untracked:Some(include_untracked),consistency:"observed_quiescence_on_recheck".into(),limitations,complete:false,scope:"bounded raw worktree and index observation; non-atomic; no authenticated provenance; recovery only to a new worktree".into()};
        let staged_entries = manifest
            .staged
            .iter()
            .enumerate()
            .filter(|(_, e)| e.mode != "160000")
            .map(|(i, e)| (format!("e{i:06}"), e.object.clone(), false))
            .collect::<Vec<_>>();
        let worktree_entries = manifest
            .worktree
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.blob.clone().map(|o| (format!("e{i:06}"), o, false)))
            .collect::<Vec<_>>();
        let staged_tree = self.checkpoint_tree(root, format, &staged_entries)?;
        let worktree_tree = self.checkpoint_tree(root, format, &worktree_entries)?;
        let manifest_json = serde_json::to_vec(&manifest)
            .map_err(|_| GitError::Parse("checkpoint manifest serialization"))?;
        if manifest_json.len() > 1_048_576 {
            return Err(GitError::Unavailable(
                "checkpoint manifest exceeds reader limit",
            ));
        }
        let manifest_oid = self.checkpoint_hash_blob(root, format, &manifest_json)?;
        let tree = self.checkpoint_tree(
            root,
            format,
            &[
                ("manifest.json".into(), manifest_oid, false),
                ("index".into(), index_blob, false),
                ("staged".into(), staged_tree, true),
                ("worktree".into(), worktree_tree, true),
            ],
        )?;
        let args = [
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit-tree",
            tree.as_hex(),
            "-p",
            base.as_hex(),
            "--no-gpg-sign",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        let env = [
            ("GIT_AUTHOR_NAME", OsString::from("RIT checkpoint")),
            ("GIT_AUTHOR_EMAIL", OsString::from("checkpoint@rit.invalid")),
            ("GIT_COMMITTER_NAME", OsString::from("RIT checkpoint")),
            (
                "GIT_COMMITTER_EMAIL",
                OsString::from("checkpoint@rit.invalid"),
            ),
        ];
        let (out, _) = self.run_internal(
            Some(root),
            &args,
            "workspace checkpoint commit",
            &env,
            false,
            Some(b"RIT content checkpoint (unauthenticated)\n"),
        )?;
        let commit = ObjectId::new(
            format,
            std::str::from_utf8(&out)
                .map_err(|_| GitError::NonUnicode)?
                .trim_end(),
        )
        .map_err(GitError::Parse)?;
        let reference = format!("refs/rit/content-checkpoints/{id}");
        let zero = "0".repeat(commit.as_hex().len());
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            reference.as_str(),
            commit.as_hex(),
            zero.as_str(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(root), &args, "workspace checkpoint ref")?;
        Ok(ContentCheckpointRecord {
            reference,
            object: commit,
            manifest,
        })
    }
    /// List and validate content checkpoint manifests and referenced Git tree entries.
    pub fn content_checkpoint_list(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::checkpoint::ContentCheckpointRecord>, GitError> {
        use rit_core::{
            checkpoint::{ContentCheckpoint, ContentCheckpointRecord},
            git_object::ObjectId,
        };
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/content-checkpoints/",
            ],
            "checkpoint content refs",
        )?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let fields = line.split('\0').collect::<Vec<_>>();
            if fields.len() != 3 || fields[2] != "commit" {
                return Err(GitError::Parse("content checkpoint ref"));
            }
            let object = ObjectId::new(format, fields[1]).map_err(GitError::Parse)?;
            let spec = format!("{}:manifest.json", object.as_hex());
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), spec.into()],
                "workspace checkpoint manifest read",
            )?;
            if raw.len() > 1_048_576 {
                return Err(GitError::Parse("checkpoint manifest too large"));
            }
            let m: ContentCheckpoint =
                serde_json::from_slice(&raw).map_err(|_| GitError::Parse("checkpoint manifest"))?;
            if !matches!(m.schema_version, 1..=3)
                || (m.schema_version >= 2 && m.consistency != "observed_quiescence_on_recheck")
                || (m.schema_version == 3
                    && (m
                        .capture_limits
                        .as_ref()
                        .is_none_or(|v| v.validate().is_err())
                        || m.created_at.is_none()
                        || m.include_untracked.is_none()))
                || m.complete
                || fields[0] != format!("refs/rit/content-checkpoints/{}", m.id)
                || m.base.algorithm() != format
                || m.index_blob.algorithm() != format
            {
                return Err(GitError::Parse("checkpoint manifest claims"));
            }
            for e in &m.staged {
                validate_checkpoint_path(&e.path)?;
                if e.object.algorithm() != format
                    || e.stage > 3
                    || !matches!(e.mode.as_str(), "100644" | "100755" | "120000" | "160000")
                {
                    return Err(GitError::Parse("checkpoint index evidence"));
                }
            }
            for e in &m.worktree {
                validate_checkpoint_path(&e.path)?;
                if !matches!(
                    e.category.as_str(),
                    "tracked" | "untracked" | "selected_ignored"
                ) || !matches!(e.kind.as_str(), "regular" | "symlink" | "deleted")
                    || e.blob.as_ref().is_some_and(|o| o.algorithm() != format)
                    || (e.kind == "deleted") != e.blob.is_none()
                {
                    return Err(GitError::Parse("checkpoint worktree evidence"));
                }
            }
            result.push(ContentCheckpointRecord {
                reference: fields[0].into(),
                object,
                manifest: m,
            });
        }
        Ok(result)
    }
    /// Verify Git object connectivity and manifest/tree consistency, not authenticity or atomic completeness.
    pub fn content_checkpoint_verify(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
    ) -> Result<rit_core::checkpoint::ContentCheckpointRecord, GitError> {
        let c = self
            .content_checkpoint_list(repo)?
            .into_iter()
            .find(|c| c.manifest.id == id)
            .ok_or(GitError::Parse("content checkpoint not found"))?;
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.commitGraph=false",
            "-c",
            "core.multiPackIndex=false",
            "-c",
            "fsck.skipList=/dev/null",
            "fsck",
            "--no-reflogs",
            "--no-progress",
            c.object.as_hex(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(repo), &args, "workspace checkpoint fsck")?;
        let parent_line = self.query(
            repo,
            &["rev-list", "--parents", "-n", "1", c.object.as_hex()],
            "checkpoint parent",
        )?;
        let parents = parent_line.split_whitespace().collect::<Vec<_>>();
        if parents.len() != 2
            || parents[0] != c.object.as_hex()
            || parents[1] != c.manifest.base.as_hex()
        {
            return Err(GitError::Parse("checkpoint base parent mismatch"));
        }
        let root = self.run(
            Some(repo),
            &["ls-tree".into(), "-z".into(), c.object.as_hex().into()],
            "workspace checkpoint root verify",
        )?;
        let mut root_entries = std::collections::BTreeMap::new();
        for row in root.split(|b| *b == 0).filter(|b| !b.is_empty()) {
            let row = std::str::from_utf8(row).map_err(|_| GitError::NonUnicode)?;
            let (meta, name) = row
                .split_once('\t')
                .ok_or(GitError::Parse("checkpoint root entry"))?;
            let mut f = meta.split(' ');
            let mode = f.next().unwrap_or("");
            let kind = f.next().unwrap_or("");
            let oid = f.next().unwrap_or("");
            if f.next().is_some()
                || root_entries
                    .insert(
                        name.to_owned(),
                        (mode.to_owned(), kind.to_owned(), oid.to_owned()),
                    )
                    .is_some()
            {
                return Err(GitError::Parse("checkpoint root fields"));
            }
        }
        if root_entries.len() != 4
            || !root_entries
                .get("manifest.json")
                .is_some_and(|(mode, kind, _)| mode == "100644" && kind == "blob")
            || !root_entries.get("index").is_some_and(|(mode, kind, oid)| {
                mode == "100644" && kind == "blob" && oid == c.manifest.index_blob.as_hex()
            })
            || !["staged", "worktree"].iter().all(|name| {
                root_entries
                    .get(*name)
                    .is_some_and(|(mode, kind, _)| mode == "040000" && kind == "tree")
            })
        {
            return Err(GitError::Parse("checkpoint root mismatch"));
        }
        let index = self.query(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}:index", c.object.as_hex()),
            ],
            "checkpoint index blob",
        )?;
        if index != c.manifest.index_blob.as_hex() {
            return Err(GitError::Parse("checkpoint index mismatch"));
        }
        for (section, expected) in [
            (
                "staged",
                c.manifest
                    .staged
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.mode != "160000")
                    .map(|(i, e)| (format!("e{i:06}"), e.object.as_hex().to_owned()))
                    .collect::<std::collections::BTreeMap<_, _>>(),
            ),
            (
                "worktree",
                c.manifest
                    .worktree
                    .iter()
                    .enumerate()
                    .filter_map(|(i, e)| {
                        e.blob
                            .as_ref()
                            .map(|o| (format!("e{i:06}"), o.as_hex().to_owned()))
                    })
                    .collect(),
            ),
        ] {
            let spec = format!("{}:{section}", c.object.as_hex());
            let bytes = self.run(
                Some(repo),
                &["ls-tree".into(), "-z".into(), spec.into()],
                "workspace checkpoint tree verify",
            )?;
            let mut actual = std::collections::BTreeMap::new();
            for row in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                let row = std::str::from_utf8(row).map_err(|_| GitError::NonUnicode)?;
                let (meta, name) = row
                    .split_once('\t')
                    .ok_or(GitError::Parse("checkpoint tree record"))?;
                let mut f = meta.split(' ');
                if f.next() != Some("100644") || f.next() != Some("blob") {
                    return Err(GitError::Parse("checkpoint tree mode"));
                }
                let oid = f.next().ok_or(GitError::Parse("checkpoint tree oid"))?;
                if f.next().is_some() || actual.insert(name.to_owned(), oid.to_owned()).is_some() {
                    return Err(GitError::Parse("checkpoint tree fields"));
                }
            }
            if actual != expected {
                return Err(GitError::Parse("checkpoint manifest/tree mismatch"));
            }
        }
        self.checkpoint_verify_index(repo, &c.manifest)?;
        Ok(c)
    }
    /// Examine captured small Git LFS pointers and gitlinks without invoking filters or fetching content.
    /// Local LFS observations are ephemeral; the checkpoint does not retain LFS payloads.
    pub fn checkpoint_supplementary_evidence(
        &self,
        repo: &Path,
        c: &rit_core::checkpoint::ContentCheckpointRecord,
    ) -> Result<rit_core::checkpoint::CheckpointSupplementaryEvidence, GitError> {
        use rit_core::checkpoint::{
            CheckpointSupplementaryEvidence, GitlinkEvidence, LfsPointerEvidence,
        };
        let mut blobs = std::collections::BTreeMap::<String, Vec<(String, String)>>::new();
        let mut gitlinks = Vec::new();
        for e in &c.manifest.staged {
            if e.mode == "160000" {
                gitlinks.push(GitlinkEvidence {
                    path: e.path.clone(),
                    stage: e.stage,
                    commit: e.object.clone(),
                    availability: "not_checked; nested content and dirty state not captured",
                });
            } else {
                blobs
                    .entry(e.object.as_hex().into())
                    .or_default()
                    .push((e.path.clone(), format!("index_stage_{}", e.stage)));
            }
        }
        for e in &c.manifest.worktree {
            if e.kind == "regular"
                && let Some(blob) = &e.blob
            {
                blobs
                    .entry(blob.as_hex().into())
                    .or_default()
                    .push((e.path.clone(), "worktree".into()));
            }
        }
        let mut pointers = Vec::new();
        if !blobs.is_empty() {
            let input = blobs.keys().map(|k| format!("{k}\n")).collect::<String>();
            let args = [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            let (out, _) = self.run_internal(
                Some(repo),
                &args,
                "workspace checkpoint lfs blob sizes",
                &[],
                false,
                Some(input.as_bytes()),
            )?;
            let text = std::str::from_utf8(&out).map_err(|_| GitError::NonUnicode)?;
            if text.lines().count() != blobs.len() {
                return Err(GitError::Parse("checkpoint LFS batch count"));
            }
            let config_args = ["config", "--get", "lfs.storage"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            let (_, config_code) = self.run_internal(
                Some(repo),
                &config_args,
                "workspace checkpoint lfs config",
                &[],
                true,
                None,
            )?;
            let common = self.inspect(repo)?.common_git_dir;
            for line in text.lines() {
                let f = line.split(' ').collect::<Vec<_>>();
                if f.len() != 3 || f[1] != "blob" {
                    return Err(GitError::Parse("checkpoint LFS blob size"));
                }
                let size = f[2]
                    .parse::<usize>()
                    .map_err(|_| GitError::Parse("checkpoint LFS blob size"))?;
                if size > 2048 {
                    continue;
                }
                let oid = rit_core::git_object::ObjectId::new(c.object.algorithm(), f[0])
                    .map_err(GitError::Parse)?;
                let content = self.conflict_blob(repo, &oid)?;
                if !content.starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
                    continue;
                }
                let parsed = parse_lfs_pointer(&content);
                for (path, source) in blobs
                    .get(f[0])
                    .ok_or(GitError::Parse("LFS object identity"))?
                {
                    let (hash, declared, status) = match &parsed {
                        Some((hash, size)) => (
                            Some(hash.clone()),
                            Some(*size),
                            if config_code == 0 {
                                "custom_storage_not_checked".into()
                            } else {
                                check_default_lfs_payload(Path::new(&common), hash, *size)
                            },
                        ),
                        None => (None, None, "malformed_pointer".into()),
                    };
                    pointers.push(LfsPointerEvidence {
                        path: path.clone(),
                        source: source.clone(),
                        sha256: hash,
                        size: declared,
                        local_status: status,
                    });
                }
            }
        }
        Ok(CheckpointSupplementaryEvidence {
            lfs_pointers: pointers,
            gitlinks,
            limitations: vec![
                "Only captured regular/staged blobs up to 2048 bytes are examined for canonical pointers",
                "LFS payloads are not included in checkpoints and can disappear immediately",
                "Custom LFS storage, network access and nested submodule content are not inspected",
                "Local LFS hash verification is limited to 64 MiB; no provenance or atomicity is established",
            ],
        })
    }
    fn checkpoint_verify_index(
        &self,
        repo: &Path,
        manifest: &rit_core::checkpoint::ContentCheckpoint,
    ) -> Result<Vec<u8>, GitError> {
        use std::io::Write;
        let inspected = self.inspect(repo)?;
        let root = Path::new(inspected.worktree.as_deref().ok_or(GitError::Unavailable(
            "checkpoint index verification requires a worktree",
        ))?);
        let index = self.conflict_blob(repo, &manifest.index_blob)?;
        let mut temp = tempfile::NamedTempFile::new().map_err(GitError::Launch)?;
        temp.write_all(&index).map_err(GitError::Launch)?;
        temp.flush().map_err(GitError::Launch)?;
        let env = [("GIT_INDEX_FILE", temp.path().as_os_str().into())];
        let args = ["rev-parse", "--shared-index-path"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (shared, _) = self.run_internal(
            Some(root),
            &args,
            "workspace restore shared-index check",
            &env,
            false,
            None,
        )?;
        if !shared.iter().all(u8::is_ascii_whitespace) {
            return Err(GitError::Unavailable(
                "split-index verification is unavailable",
            ));
        }
        let args = ["ls-files", "--stage", "-z"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (actual, _) = self.run_internal(
            Some(root),
            &args,
            "workspace restore index check",
            &env,
            false,
            None,
        )?;
        let mut expected = Vec::new();
        for e in &manifest.staged {
            expected.extend_from_slice(
                format!("{} {} {}\t{}\0", e.mode, e.object.as_hex(), e.stage, e.path).as_bytes(),
            );
        }
        if actual != expected {
            return Err(GitError::Parse(
                "captured index entries disagree with raw index",
            ));
        }
        Ok(index)
    }
    fn checkpoint_restore_prepare(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
    ) -> Result<PreparedRestore, GitError> {
        use rit_core::checkpoint::RestorePlan;
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "restore unsupported on this platform",
            ));
        }
        let c = self.content_checkpoint_verify(repo, id)?;
        if c.manifest.staged.iter().any(|e| e.mode == "160000") {
            return Err(GitError::Unavailable(
                "submodule checkpoint restoration is not implemented",
            ));
        }
        let target = workspace_target(dest)?;
        let ins = self.inspect(repo)?;
        let source = Path::new(
            ins.worktree
                .as_deref()
                .ok_or(GitError::Unavailable("bare repository"))?,
        );
        let common = Path::new(&ins.common_git_dir);
        if target.starts_with(source)
            || source.starts_with(&target)
            || target.starts_with(common)
            || common.starts_with(&target)
        {
            return Err(GitError::Parse("restore destination overlaps repository"));
        }
        for w in self.worktrees(repo)? {
            let p = Path::new(&w.path);
            if target.starts_with(p) || p.starts_with(&target) {
                return Err(GitError::Parse("restore destination overlaps worktree"));
            }
        }
        let index = self.checkpoint_verify_index(repo, &c.manifest)?;
        let mut files = Vec::new();
        let mut create_paths = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut total = 0usize;
        let limits = c.manifest.capture_limits.clone().unwrap_or_default();
        limits.validate().map_err(GitError::Parse)?;
        for e in &c.manifest.worktree {
            validate_checkpoint_path(&e.path)?;
            if !seen.insert(e.path.clone()) {
                return Err(GitError::Parse("duplicate restore path"));
            }
            if e.kind == "deleted" {
                if e.category != "tracked" || e.blob.is_some() {
                    return Err(GitError::Parse("invalid deleted path"));
                }
                deleted_paths.push(e.path.clone());
                continue;
            }
            if e.kind == "symlink" && e.executable {
                return Err(GitError::Parse("invalid symlink mode"));
            }
            let blob = e
                .blob
                .as_ref()
                .ok_or(GitError::Parse("missing restore blob"))?;
            let bytes = self.bounded_blob(repo, blob, limits.max_file_bytes)?;
            if e.kind == "symlink" && (bytes.is_empty() || bytes.contains(&0)) {
                return Err(GitError::Parse("invalid symlink target"));
            }
            total = total
                .checked_add(bytes.len())
                .ok_or(GitError::OutputLimit)?;
            if total > limits.max_total_bytes || files.len() >= limits.max_paths {
                return Err(GitError::Unavailable(
                    "restore exceeds bounded snapshot limits",
                ));
            }
            create_paths.push(e.path.clone());
            files.push((e.path.clone(), e.kind.clone(), e.executable, bytes));
        }
        let mut limitations = c.manifest.limitations.clone();
        limitations.push("Restores only captured bytes to an exclusively created new detached worktree; incomplete, non-atomic, unauthenticated".into());
        limitations.push(
            "LFS pointers are restored as recorded bytes only; LFS payloads are not recovered"
                .into(),
        );
        limitations.push(
            "Git operation state such as MERGE_HEAD, rebase metadata and reflogs is not captured"
                .into(),
        );
        limitations.push("Symlink targets are recreated verbatim and may point outside the worktree; they are never followed during restore".into());
        Ok((
            RestorePlan {
                checkpoint: c,
                destination: target.to_str().ok_or(GitError::NonUnicode)?.into(),
                create_paths,
                deleted_paths,
                limitations,
            },
            index,
            files,
        ))
    }
    /// Verify checkpoint and preview an exclusive new-worktree recovery, without writing Git refs or files.
    pub fn checkpoint_restore_plan(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
    ) -> Result<rit_core::checkpoint::RestorePlan, GitError> {
        Ok(self.checkpoint_restore_prepare(repo, id, dest)?.0)
    }
    /// Restore the captured subset only into a newly created detached worktree; never overwrite existing paths.
    pub fn checkpoint_restore_new(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
    ) -> Result<rit_core::checkpoint::RestoreObservation, GitError> {
        use rit_core::checkpoint::{RestoreId, RestoreOperation};
        let (plan, index, files) = self.checkpoint_restore_prepare(repo, id, dest)?;
        let format = plan.checkpoint.object.algorithm();
        let rid = RestoreId::new();
        let zero = "0".repeat(plan.checkpoint.object.as_hex().len());
        let reference = format!("refs/rit/restores/{rid}/manifest");
        let hold = format!("refs/rit/restores/{rid}/checkpoint");
        let mut op = RestoreOperation {
            schema_version: 1,
            id: rid,
            checkpoint: id,
            checkpoint_object: plan.checkpoint.object.clone(),
            base: plan.checkpoint.manifest.base.clone(),
            destination: plan.destination.clone(),
            state: "created".into(),
            completed_paths: Vec::new(),
            current_path: None,
            failure: None,
            action:
                "inspect actual worktree and files; never automatically replay, overwrite or remove"
                    .into(),
        };
        let mut oid = self.write_json_ref(repo, format, &reference, &op, &zero)?;
        let action = (|| -> Result<(), GitError> {
            let args = [
                "-c",
                "core.hooksPath=/dev/null",
                "update-ref",
                hold.as_str(),
                op.checkpoint_object.as_hex(),
                zero.as_str(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            self.run(Some(repo), &args, "workspace restore retain checkpoint")?;
            op.state = "checkpoint_retained".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            let target = Path::new(&op.destination);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(target)
                    .map_err(|source| GitError::Path {
                        path: target.into(),
                        source,
                    })?;
            }
            #[cfg(not(unix))]
            {
                return Err(GitError::Unavailable("restore path unavailable"));
            }
            op.state = "directory_created".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            let args = vec![
                "-c".into(),
                "core.hooksPath=/dev/null".into(),
                "worktree".into(),
                "add".into(),
                "--no-checkout".into(),
                "--detach".into(),
                target.as_os_str().into(),
                op.base.as_hex().into(),
            ];
            self.run(Some(repo), &args, "workspace restore worktree add")?;
            let actual = self
                .worktrees(repo)?
                .into_iter()
                .find(|w| w.path == op.destination)
                .ok_or(GitError::Parse("restore worktree missing"))?;
            if !actual.detached || actual.head.as_ref() != Some(&op.base) {
                return Err(GitError::Parse("restore worktree mismatch"));
            }
            op.state = "worktree_created".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            for (path, kind, executable, bytes) in &files {
                op.current_path = Some(path.clone());
                oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
                restore_create_entry(target, path, kind, *executable, bytes)?;
                op.completed_paths.push(path.clone());
                op.current_path = None;
                oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            }
            op.state = "files_created".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            let gitdir = self.query(
                target,
                &["rev-parse", "--absolute-git-dir"],
                "workspace restore gitdir",
            )?;
            let dir = Path::new(&gitdir)
                .canonicalize()
                .map_err(GitError::Launch)?;
            let common = Path::new(&self.inspect(repo)?.common_git_dir)
                .join("worktrees")
                .canonicalize()
                .map_err(GitError::Launch)?;
            if dir.parent() != Some(common.as_path()) {
                return Err(GitError::Parse("unexpected restore gitdir"));
            }
            restore_create_index(&dir, &index)?;
            op.state = "index_created".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            if read_worktree_file(&dir, "index")?.as_deref() != Some(index.as_slice()) {
                return Err(GitError::Parse("restored index verification failed"));
            }
            for (path, kind, executable, bytes) in &files {
                let actual = read_checkpoint_entry(
                    target,
                    path,
                    plan.checkpoint
                        .manifest
                        .capture_limits
                        .as_ref()
                        .map_or(1_048_576, |l| l.max_file_bytes),
                )?
                .ok_or(GitError::Parse("restored path disappeared"))?;
                if actual.0 != kind || actual.1 != *executable || actual.2 != *bytes {
                    return Err(GitError::Parse("restored path verification failed"));
                }
            }
            for path in &plan.deleted_paths {
                if read_checkpoint_entry(
                    target,
                    path,
                    plan.checkpoint
                        .manifest
                        .capture_limits
                        .as_ref()
                        .map_or(1_048_576, |l| l.max_file_bytes),
                )?
                .is_some()
                {
                    return Err(GitError::Parse("deleted path unexpectedly exists"));
                }
            }
            op.state = "completed".into();
            op.action =
                "captured subset verified once; inspect before use, it may change immediately"
                    .into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            Ok(())
        })();
        if let Err(e) = action {
            op.failure = Some(format!("{}: {e}", op.state));
            let _result = self.write_json_ref(repo, format, &reference, &op, oid.as_hex());
            return Err(GitError::Provisioning {
                id: rid.to_string(),
            });
        }
        self.checkpoint_restore_list(repo)?
            .into_iter()
            .find(|x| x.operation.id == rid)
            .ok_or(GitError::Parse("restore operation missing"))
    }
    /// List restore intents and reconcile them with Git inventory without changing anything.
    pub fn checkpoint_restore_list(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::checkpoint::RestoreObservation>, GitError> {
        use rit_core::{
            checkpoint::{RestoreObservation, RestoreOperation},
            git_object::ObjectId,
        };
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/restores/",
            ],
            "workspace restore refs",
        )?;
        let trees = self.worktrees(repo)?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 {
                return Err(GitError::Parse("restore ref"));
            }
            if f[0].ends_with("/checkpoint") {
                continue;
            }
            if f[2] != "blob" {
                return Err(GitError::Parse("restore manifest ref"));
            }
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace restore manifest",
            )?;
            if raw.len() > 1_048_576 {
                return Err(GitError::OutputLimit);
            }
            let op: RestoreOperation =
                serde_json::from_slice(&raw).map_err(|_| GitError::Parse("restore manifest"))?;
            if op.schema_version != 1
                || f[0] != format!("refs/rit/restores/{}/manifest", op.id)
                || op.base.algorithm() != format
                || op.checkpoint_object.algorithm() != format
                || !Path::new(&op.destination).is_absolute()
                || !matches!(
                    op.state.as_str(),
                    "created"
                        | "checkpoint_retained"
                        | "directory_created"
                        | "worktree_created"
                        | "files_created"
                        | "index_created"
                        | "completed"
                )
            {
                return Err(GitError::Parse("restore operation claims"));
            }
            for p in &op.completed_paths {
                validate_checkpoint_path(p)?
            }
            if let Some(p) = &op.current_path {
                validate_checkpoint_path(p)?
            }
            let pin = format!("refs/rit/restores/{}/checkpoint", op.id);
            let pinned = refs.lines().any(|line| {
                let fields = line.split('\0').collect::<Vec<_>>();
                fields.len() == 3
                    && fields[0] == pin
                    && fields[1] == op.checkpoint_object.as_hex()
                    && fields[2] == "commit"
            });
            let worktree = trees.iter().find(|w| w.path == op.destination).cloned();
            let observation = if !pinned {
                "checkpoint_ref_missing_or_mismatch"
            } else if worktree.is_none() {
                "worktree_missing_or_incomplete"
            } else if worktree
                .as_ref()
                .is_some_and(|w| !w.detached || w.head.as_ref() != Some(&op.base))
            {
                "worktree_mismatch"
            } else if op.state != "completed" {
                "interrupted_manual_recovery_required"
            } else {
                "completed_inventory_only"
            };
            result.push(RestoreObservation {
                operation: op,
                object: ObjectId::new(format, f[1]).map_err(GitError::Parse)?,
                worktree,
                observation: observation.into(),
            });
        }
        Ok(result)
    }
    fn checkpoint_extract_prepare(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
        requested: &[String],
    ) -> Result<PreparedExtraction, GitError> {
        if requested.is_empty() || requested.len() > 100 {
            return Err(GitError::Parse(
                "select 1..100 exact files or directory prefixes",
            ));
        }
        let (full, _index, files) = self.checkpoint_restore_prepare(repo, id, dest)?;
        let mut selectors = std::collections::BTreeSet::new();
        for raw in requested {
            let p = raw.trim_end_matches('/');
            validate_checkpoint_path(p)?;
            if !selectors.insert(p.to_owned()) {
                return Err(GitError::Parse("duplicate extraction selector"));
            }
        }
        let matches = |path: &str, s: &str| {
            path == s || path.strip_prefix(s).is_some_and(|r| r.starts_with('/'))
        };
        for s in &selectors {
            if !full
                .create_paths
                .iter()
                .chain(full.deleted_paths.iter())
                .any(|p| matches(p, s))
            {
                return Err(GitError::Parse(
                    "extraction selector does not match captured paths",
                ));
            }
        }
        let chosen = files
            .into_iter()
            .filter(|(p, _, _, _)| selectors.iter().any(|s| matches(p, s)))
            .collect::<Vec<_>>();
        if chosen.is_empty() {
            return Err(GitError::Unavailable(
                "selected paths contain only recorded deletions; no content to extract",
            ));
        }
        let selected_paths = chosen.iter().map(|(p, _, _, _)| p.clone()).collect();
        let deletions = full
            .deleted_paths
            .into_iter()
            .filter(|p| selectors.iter().any(|s| matches(p, s)))
            .collect();
        Ok((rit_core::checkpoint::ExtractionPlan{checkpoint:full.checkpoint,destination:full.destination,selected_paths,recorded_deletions:deletions,limitations:vec!["Selected captured raw worktree bytes only; index, staged versions and Git operation state are not extracted".into(),"Destination must be new; existing paths and the original worktree are never intentionally overwritten".into(),"Capture is incomplete, non-atomic and unauthenticated; excluded content cannot be recovered".into(),"Symlink targets are recreated verbatim and may point outside the destination; never followed by RIT".into()]},chosen))
    }
    /// Preview extraction of selected recorded files to a new directory.
    pub fn checkpoint_extract_plan(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
        requested: &[String],
    ) -> Result<rit_core::checkpoint::ExtractionPlan, GitError> {
        Ok(self
            .checkpoint_extract_prepare(repo, id, dest, requested)?
            .0)
    }
    /// Extract selected captured bytes to a new directory, with a persistent cooperative intent.
    pub fn checkpoint_extract_new(
        &self,
        repo: &Path,
        id: rit_core::checkpoint::CheckpointId,
        dest: &Path,
        requested: &[String],
    ) -> Result<rit_core::checkpoint::ExtractionObservation, GitError> {
        use rit_core::checkpoint::{ExtractionOperation, RestoreId};
        let (plan, files) = self.checkpoint_extract_prepare(repo, id, dest, requested)?;
        let format = plan.checkpoint.object.algorithm();
        let rid = RestoreId::new();
        let zero = "0".repeat(plan.checkpoint.object.as_hex().len());
        let reference = format!("refs/rit/extractions/{rid}/manifest");
        let hold = format!("refs/rit/extractions/{rid}/checkpoint");
        let mut op=ExtractionOperation{schema_version:1,id:rid,checkpoint:id,checkpoint_object:plan.checkpoint.object.clone(),destination:plan.destination,selected_paths:plan.selected_paths,completed_paths:Vec::new(),current_path:None,state:"created".into(),failure:None,action:"inspect destination and operation before retry; never automatically overwrite or remove".into()};
        let mut oid = self.write_json_ref(repo, format, &reference, &op, &zero)?;
        let result = (|| -> Result<(), GitError> {
            let args = [
                "-c",
                "core.hooksPath=/dev/null",
                "update-ref",
                hold.as_str(),
                op.checkpoint_object.as_hex(),
                zero.as_str(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            self.run(Some(repo), &args, "workspace extraction retain checkpoint")?;
            op.state = "checkpoint_retained".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&op.destination)
                    .map_err(|source| GitError::Path {
                        path: PathBuf::from(&op.destination),
                        source,
                    })?;
            }
            #[cfg(not(unix))]
            {
                return Err(GitError::Unavailable("extraction unsupported"));
            }
            op.state = "directory_created".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            for (path, kind, exec, bytes) in &files {
                op.current_path = Some(path.clone());
                op.state = "writing".into();
                oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
                restore_create_entry(Path::new(&op.destination), path, kind, *exec, bytes)?;
                op.completed_paths.push(path.clone());
                op.current_path = None;
                oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            }
            let limit = plan
                .checkpoint
                .manifest
                .capture_limits
                .as_ref()
                .map_or(1_048_576, |l| l.max_file_bytes);
            for (path, kind, exec, bytes) in &files {
                let actual = read_checkpoint_entry(Path::new(&op.destination), path, limit)?
                    .ok_or(GitError::Parse("extracted path missing"))?;
                if actual.0 != kind || actual.1 != *exec || actual.2 != *bytes {
                    return Err(GitError::Parse("extraction verification failed"));
                }
            }
            op.state = "completed".into();
            op.action = "selected content verified once; it may change immediately".into();
            oid = self.write_json_ref(repo, format, &reference, &op, oid.as_hex())?;
            Ok(())
        })();
        if let Err(e) = result {
            op.failure = Some(format!("{}: {e}", op.state));
            let _recorded = self.write_json_ref(repo, format, &reference, &op, oid.as_hex());
            return Err(GitError::Provisioning {
                id: rid.to_string(),
            });
        }
        self.checkpoint_extractions(repo)?
            .into_iter()
            .find(|v| v.operation.id == rid)
            .ok_or(GitError::Parse("extraction operation missing"))
    }
    /// Discover extraction intents without replay or deletion.
    pub fn checkpoint_extractions(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::checkpoint::ExtractionObservation>, GitError> {
        use rit_core::{
            checkpoint::{ExtractionObservation, ExtractionOperation},
            git_object::ObjectId,
        };
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/extractions/",
            ],
            "workspace extraction refs",
        )?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 {
                return Err(GitError::Parse("extraction ref"));
            }
            if f[0].ends_with("/checkpoint") {
                continue;
            }
            if f[2] != "blob" {
                return Err(GitError::Parse("extraction manifest ref"));
            }
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace extraction manifest",
            )?;
            if raw.len() > 1_048_576 {
                return Err(GitError::OutputLimit);
            }
            let op: ExtractionOperation =
                serde_json::from_slice(&raw).map_err(|_| GitError::Parse("extraction manifest"))?;
            if op.schema_version != 1
                || f[0] != format!("refs/rit/extractions/{}/manifest", op.id)
                || op.checkpoint_object.algorithm() != format
                || !Path::new(&op.destination).is_absolute()
                || op.selected_paths.len() > 2000
                || op.completed_paths.len() > op.selected_paths.len()
                || !matches!(
                    op.state.as_str(),
                    "created"
                        | "checkpoint_retained"
                        | "directory_created"
                        | "writing"
                        | "completed"
                )
            {
                return Err(GitError::Parse("extraction manifest claims"));
            }
            for p in op
                .selected_paths
                .iter()
                .chain(op.completed_paths.iter())
                .chain(op.current_path.iter())
            {
                validate_checkpoint_path(p)?
            }
            let pin = format!("refs/rit/extractions/{}/checkpoint", op.id);
            let pinned = refs.lines().any(|line| {
                let fields = line.split('\0').collect::<Vec<_>>();
                fields.len() == 3
                    && fields[0] == pin
                    && fields[1] == op.checkpoint_object.as_hex()
                    && fields[2] == "commit"
            });
            let dir = std::fs::symlink_metadata(&op.destination)
                .ok()
                .is_some_and(|m| m.file_type().is_dir());
            let observation = if !pinned {
                "checkpoint_ref_missing_or_mismatch"
            } else if !dir {
                "destination_missing_or_not_directory"
            } else if op.state != "completed" {
                "interrupted_manual_recovery_required"
            } else {
                "completed_directory_inventory_only"
            };
            result.push(ExtractionObservation {
                operation: op,
                object: ObjectId::new(format, f[1]).map_err(GitError::Parse)?,
                observation: observation.into(),
            });
        }
        Ok(result)
    }
    fn write_json_ref<T: serde::Serialize>(
        &self,
        repo: &Path,
        format: rit_core::git_object::ObjectFormat,
        reference: &str,
        value: &T,
        old: &str,
    ) -> Result<rit_core::git_object::ObjectId, GitError> {
        let bytes =
            serde_json::to_vec(value).map_err(|_| GitError::Parse("metadata serialization"))?;
        if bytes.len() > 1_048_576 {
            return Err(GitError::OutputLimit);
        }
        let oid = self.checkpoint_hash_blob(repo, format, &bytes)?;
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            reference,
            oid.as_hex(),
            old,
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(repo), &args, "workspace metadata ref")?;
        Ok(oid)
    }
    fn resolve_commit(
        &self,
        repo: &Path,
        format: rit_core::git_object::ObjectFormat,
        revision: &str,
    ) -> Result<rit_core::git_object::ObjectId, GitError> {
        if revision.is_empty() || revision.len() > 256 || revision.contains('\0') {
            return Err(GitError::Parse("invalid revision"));
        }
        let hex = self.query(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
            "revision",
        )?;
        rit_core::git_object::ObjectId::new(format, &hex).map_err(GitError::Parse)
    }
    /// Record a clean managed workspace's committed source and target as unauthenticated Git metadata.
    pub fn submission_create(
        &self,
        repo: &Path,
        id: rit_core::workspace::WorkspaceId,
        target: &str,
    ) -> Result<rit_core::submission::GitSubmission, GitError> {
        use rit_core::submission::{GitSubmission, SubmissionId};
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "submissions not validated on this platform",
            ));
        }
        if !target.starts_with("refs/heads/") || target.contains('\0') || target.len() > 256 {
            return Err(GitError::Parse("target must be a full local branch ref"));
        }
        self.run(
            Some(repo),
            &["check-ref-format".into(), target.into()],
            "workspace target ref",
        )?;
        let state = self.workspace_reconcile(repo)?;
        let workspace = state
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == id && w.observation == "present")
            .ok_or(GitError::Unavailable("workspace not present and ready"))?;
        let status = workspace
            .status
            .ok_or(GitError::Unavailable("workspace status unavailable"))?;
        if !status.changes.is_empty() {
            return Err(GitError::Unavailable(
                "submit requires a clean workspace; commit or checkpoint uncommitted work first",
            ));
        }
        let format = self.object_format(repo)?;
        let source = self.resolve_commit(repo, format, &workspace.manifest.branch)?;
        if workspace.worktree.as_ref().and_then(|w| w.head.as_ref()) != Some(&source) {
            return Err(GitError::Unavailable(
                "source branch moved during submission",
            ));
        }
        let target_commit = self.resolve_commit(repo, format, target)?;
        let args = [
            "diff-tree",
            "-r",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            workspace.manifest.base.as_hex(),
            source.as_hex(),
            "--",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        let changed = self
            .run(Some(repo), &args, "workspace submission diff")?
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                std::str::from_utf8(b)
                    .map(str::to_owned)
                    .map_err(|_| GitError::NonUnicode)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sid = SubmissionId::new();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| GitError::Parse("clock before epoch"))?
            .as_secs();
        let item=GitSubmission{schema_version:2,id:sid,workspace:id,source_branch:workspace.manifest.branch,source_commit:source,base_commit:workspace.manifest.base,target_ref:target.into(),target_commit,changed_files:changed,lifecycle:"submitted".into(),created_at:Some(created_at),scope:"committed source only; unauthenticated; ignored and later worktree changes excluded".into()};
        let reference = format!("refs/rit/submissions/{sid}/manifest");
        let zero = "0".repeat(item.source_commit.as_hex().len());
        self.write_json_ref(repo, format, &reference, &item, &zero)?;
        Ok(item)
    }
    /// Rediscover Git-ref-backed, unauthenticated submissions.
    pub fn submission_list(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::submission::GitSubmission>, GitError> {
        use rit_core::submission::GitSubmission;
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/submissions/",
            ],
            "submission refs",
        )?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 || f[2] != "blob" {
                return Err(GitError::Parse("submission ref"));
            }
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace submission read",
            )?;
            if raw.len() > 1_048_576 {
                return Err(GitError::OutputLimit);
            }
            let item: GitSubmission =
                serde_json::from_slice(&raw).map_err(|_| GitError::Parse("submission manifest"))?;
            if !matches!(item.schema_version, 1 | 2)
                || !matches!(item.lifecycle.as_str(), "submitted" | "cancelled")
                || f[0] != format!("refs/rit/submissions/{}/manifest", item.id)
                || item.source_branch != format!("refs/heads/rit/workspaces/{}", item.workspace)
                || !item.target_ref.starts_with("refs/heads/")
                || item.source_commit.algorithm() != format
                || item.target_commit.algorithm() != format
                || item.base_commit.algorithm() != format
            {
                return Err(GitError::Parse("submission claims"));
            }
            self.run(
                Some(repo),
                &["check-ref-format".into(), item.target_ref.as_str().into()],
                "workspace submission validate",
            )?;
            for p in &item.changed_files {
                validate_checkpoint_path(p)?
            }
            result.push(item);
        }
        Ok(result)
    }
    /// CAS-cancel a cooperative submission without deleting candidates or changing source/target refs.
    pub fn submission_cancel(
        &self,
        repo: &Path,
        id: rit_core::submission::SubmissionId,
    ) -> Result<rit_core::submission::GitSubmission, GitError> {
        let format = self.object_format(repo)?;
        let reference = format!("refs/rit/submissions/{id}/manifest");
        let before = self.query(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                reference.as_str(),
            ],
            "workspace submission ref",
        )?;
        let mut item = self
            .submission_list(repo)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or(GitError::Parse("submission not found"))?;
        let after = self.query(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                reference.as_str(),
            ],
            "workspace submission ref",
        )?;
        if before != after {
            return Err(GitError::Unavailable(
                "submission changed during cancellation",
            ));
        }
        if item.lifecycle == "cancelled" {
            return Ok(item);
        }
        if item.lifecycle != "submitted" {
            return Err(GitError::Unavailable(
                "submission state cannot be cancelled",
            ));
        }
        item.lifecycle = "cancelled".into();
        item.schema_version = 2;
        self.write_json_ref(repo, format, &reference, &item, &before)?;
        Ok(item)
    }
    /// Recheck submission source and target against live Git before further work.
    pub fn submission_validate(
        &self,
        repo: &Path,
        item: &rit_core::submission::GitSubmission,
    ) -> Result<(), GitError> {
        if item.lifecycle != "submitted" {
            return Err(GitError::Unavailable("submission is not active"));
        }
        let format = self.object_format(repo)?;
        let workspace = self
            .workspace_reconcile(repo)?
            .workspaces
            .into_iter()
            .find(|w| w.manifest.id == item.workspace && w.observation == "present")
            .ok_or(GitError::Unavailable("source workspace is unavailable"))?;
        if workspace.manifest.branch != item.source_branch
            || self.resolve_commit(repo, format, &item.source_branch)? != item.source_commit
        {
            return Err(GitError::Unavailable(
                "source branch moved since submission",
            ));
        }
        if self.resolve_commit(repo, format, &item.target_ref)? != item.target_commit {
            return Err(GitError::Unavailable("target moved since submission"));
        }
        Ok(())
    }
    /// Construct a detached candidate worktree and merge commit without updating the target ref.
    /// Candidate metadata is Git-backed coordination only; builds/tests and publication are disabled.
    pub fn candidate_create(
        &self,
        repo: &Path,
        submission: rit_core::submission::SubmissionId,
        dest: &Path,
    ) -> Result<rit_core::submission::CandidateObservation, GitError> {
        use rit_core::{
            git_object::ObjectId,
            submission::{CandidateId, GitCandidate},
        };
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(GitError::Unavailable(
                "candidate construction not validated on this platform",
            ));
        }
        let item = self
            .submission_list(repo)?
            .into_iter()
            .find(|s| s.id == submission)
            .ok_or(GitError::Parse("submission not found"))?;
        let format = self.object_format(repo)?;
        self.submission_validate(repo, &item)?;
        if item.source_commit == item.target_commit {
            return Err(GitError::Unavailable(
                "source and target commits are identical; no integration candidate needed",
            ));
        }
        let ins = self.inspect(repo)?;
        let target_path = workspace_target(dest)?;
        let source = Path::new(
            ins.worktree
                .as_deref()
                .ok_or(GitError::Unavailable("bare candidate repository"))?,
        );
        let common = Path::new(&ins.common_git_dir);
        if target_path.starts_with(source)
            || source.starts_with(&target_path)
            || target_path.starts_with(common)
            || common.starts_with(&target_path)
        {
            return Err(GitError::Parse("candidate path overlaps repository"));
        }
        for w in self.worktrees(repo)? {
            let p = Path::new(&w.path);
            if target_path.starts_with(p) || p.starts_with(&target_path) {
                return Err(GitError::Parse("candidate path overlaps worktree"));
            }
        }
        let sim = self.simulate_merge_mode(
            repo,
            item.target_commit.as_hex(),
            item.source_commit.as_hex(),
            true,
        )?;
        if sim.has_conflicts {
            return Err(GitError::Unavailable(
                "candidate merge conflicts; run conflicts simulate",
            ));
        }
        if self.resolve_commit(repo, format, &item.target_ref)? != item.target_commit {
            return Err(GitError::Unavailable(
                "target moved during candidate simulation",
            ));
        }
        let fresh = self
            .submission_list(repo)?
            .into_iter()
            .find(|v| v.id == submission)
            .ok_or(GitError::Unavailable(
                "submission removed during candidate creation",
            ))?;
        if fresh.lifecycle != "submitted"
            || fresh.source_commit != item.source_commit
            || fresh.target_commit != item.target_commit
            || fresh.target_ref != item.target_ref
        {
            return Err(GitError::Unavailable(
                "submission changed during candidate creation",
            ));
        }
        self.submission_validate(repo, &fresh)?;
        let args = [
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit-tree",
            sim.ephemeral_tree.as_hex(),
            "-p",
            item.target_commit.as_hex(),
            "-p",
            item.source_commit.as_hex(),
            "--no-gpg-sign",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        let env = [
            ("GIT_AUTHOR_NAME", OsString::from("RIT candidate")),
            ("GIT_AUTHOR_EMAIL", OsString::from("candidate@rit.invalid")),
            ("GIT_COMMITTER_NAME", OsString::from("RIT candidate")),
            (
                "GIT_COMMITTER_EMAIL",
                OsString::from("candidate@rit.invalid"),
            ),
        ];
        let (out, _) = self.run_internal(
            Some(repo),
            &args,
            "workspace candidate commit",
            &env,
            false,
            Some(b"RIT integration candidate (unauthenticated)\n"),
        )?;
        let commit = ObjectId::new(
            format,
            std::str::from_utf8(&out)
                .map_err(|_| GitError::NonUnicode)?
                .trim_end(),
        )
        .map_err(GitError::Parse)?;
        let cid = CandidateId::new();
        let mut c = GitCandidate {
            schema_version: 1,
            id: cid,
            submission,
            commit: commit.clone(),
            source_commit: item.source_commit,
            target_ref: item.target_ref,
            target_commit: item.target_commit,
            path: target_path.to_str().ok_or(GitError::NonUnicode)?.into(),
            state: "provisioning".into(),
            limitation: None,
        };
        let manifest_ref = format!("refs/rit/candidates/{cid}/manifest");
        let commit_ref = format!("refs/rit/candidates/{cid}/commit");
        let zero = "0".repeat(commit.as_hex().len());
        let mut old = self.write_json_ref(repo, format, &manifest_ref, &c, &zero)?;
        let result = (|| -> Result<(), GitError> {
            let args = [
                "-c",
                "core.hooksPath=/dev/null",
                "update-ref",
                commit_ref.as_str(),
                commit.as_hex(),
                zero.as_str(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            self.run(Some(repo), &args, "workspace candidate ref")?;
            c.state = "commit_reserved".into();
            old = self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&target_path)
                    .map_err(|source| GitError::Path {
                        path: target_path.clone(),
                        source,
                    })?;
            }
            #[cfg(not(unix))]
            {
                return Err(GitError::Unavailable("candidate path unavailable"));
            }
            c.state = "directory_created".into();
            old = self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex())?;
            let args = vec![
                "-c".into(),
                "core.hooksPath=/dev/null".into(),
                "worktree".into(),
                "add".into(),
                "--no-checkout".into(),
                "--detach".into(),
                target_path.as_os_str().into(),
                commit.as_hex().into(),
            ];
            self.run(Some(repo), &args, "workspace candidate worktree")?;
            c.state = "worktree_created".into();
            old = self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex())?;
            let actual = self
                .worktrees(repo)?
                .into_iter()
                .find(|w| w.path == c.path)
                .ok_or(GitError::Parse("candidate worktree missing"))?;
            if !actual.detached || actual.head.as_ref() != Some(&commit) {
                return Err(GitError::Parse("candidate worktree mismatch"));
            }
            c.state = "worktree_verified".into();
            old = self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex())?;
            match self.workspace_materialize(&target_path, &commit) {
                Ok(()) => c.state = "ready".into(),
                Err(GitError::Unavailable(reason)) => {
                    c.state = "no_checkout".into();
                    c.limitation = Some(reason.into())
                }
                Err(e) => return Err(e),
            }
            self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex())?;
            Ok(())
        })();
        if let Err(e) = result {
            c.limitation = Some(format!(
                "failure after {}: {e}; inspect manually, do not auto-clean",
                c.state
            ));
            let _recorded = self.write_json_ref(repo, format, &manifest_ref, &c, old.as_hex());
            return Err(GitError::Provisioning {
                id: cid.to_string(),
            });
        }
        self.candidate_list(repo)?
            .into_iter()
            .find(|x| x.candidate.id == cid)
            .ok_or(GitError::Parse("candidate missing after creation"))
    }
    /// Reconcile candidate manifests against Git worktrees and target branch values.
    pub fn candidate_list(
        &self,
        repo: &Path,
    ) -> Result<Vec<rit_core::submission::CandidateObservation>, GitError> {
        use rit_core::submission::{CandidateObservation, GitCandidate};
        let format = self.object_format(repo)?;
        let refs = self.query(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/candidates/",
            ],
            "candidate refs",
        )?;
        let trees = self.worktrees(repo)?;
        let mut result = Vec::new();
        for line in refs.lines() {
            let f = line.split('\0').collect::<Vec<_>>();
            if f.len() != 3 {
                return Err(GitError::Parse("candidate ref"));
            }
            if f[0].ends_with("/commit") {
                continue;
            }
            if f[2] != "blob" {
                return Err(GitError::Parse("candidate manifest ref"));
            }
            let raw = self.run(
                Some(repo),
                &["cat-file".into(), "blob".into(), f[1].into()],
                "workspace candidate read",
            )?;
            let c: GitCandidate =
                serde_json::from_slice(&raw).map_err(|_| GitError::Parse("candidate manifest"))?;
            if c.schema_version != 1
                || f[0] != format!("refs/rit/candidates/{}/manifest", c.id)
                || c.commit.algorithm() != format
                || c.source_commit.algorithm() != format
                || c.target_commit.algorithm() != format
                || !c.target_ref.starts_with("refs/heads/")
                || !Path::new(&c.path).is_absolute()
                || !matches!(
                    c.state.as_str(),
                    "ready"
                        | "no_checkout"
                        | "provisioning"
                        | "commit_reserved"
                        | "directory_created"
                        | "worktree_created"
                        | "worktree_verified"
                )
            {
                return Err(GitError::Parse("candidate claims"));
            }
            self.run(
                Some(repo),
                &["check-ref-format".into(), c.target_ref.as_str().into()],
                "workspace candidate target",
            )?;
            let commit_ref = format!("refs/rit/candidates/{}/commit", c.id);
            let ref_matches = refs.lines().any(|line| {
                let fields = line.split('\0').collect::<Vec<_>>();
                fields.len() == 3
                    && fields[0] == commit_ref
                    && fields[1] == c.commit.as_hex()
                    && fields[2] == "commit"
            });
            let worktree = trees.iter().find(|w| w.path == c.path).cloned();
            let observation = match &worktree {
                None if !matches!(c.state.as_str(), "ready" | "no_checkout") => {
                    "interrupted_missing_worktree"
                }
                None => "missing",
                Some(_) if !matches!(c.state.as_str(), "ready" | "no_checkout") => "interrupted",
                Some(_) if !ref_matches => "candidate_ref_mismatch",
                Some(w) if !w.detached || w.head.as_ref() != Some(&c.commit) => "mismatch",
                Some(_) if c.state == "no_checkout" => "no_checkout",
                Some(_) => "present",
            }
            .to_string();
            let target_moved = self
                .resolve_commit(repo, format, &c.target_ref)
                .map_or(true, |v| v != c.target_commit);
            result.push(CandidateObservation {
                candidate: c,
                worktree,
                observation,
                target_moved,
            });
        }
        Ok(result)
    }
    /// Run limited Git object-integrity checks without executing hooks or tests.
    pub fn candidate_verify_objects(
        &self,
        repo: &Path,
        commit: &rit_core::git_object::ObjectId,
    ) -> Result<(), GitError> {
        let args = [
            "-c",
            "core.commitGraph=false",
            "-c",
            "core.multiPackIndex=false",
            "-c",
            "fsck.skipList=/dev/null",
            "fsck",
            "--strict",
            "--no-reflogs",
            "--no-dangling",
            commit.as_hex(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(repo), &args, "workspace candidate fsck")?;
        Ok(())
    }
    /// Create one commit-only Git checkpoint ref with an expected nonexistent old value.
    /// This does not capture or alter the index, working tree, untracked files or submodule contents.
    pub fn checkpoint_commit(
        &self,
        input: &Path,
    ) -> Result<rit_core::checkpoint::CommitCheckpoint, GitError> {
        use rit_core::{
            checkpoint::{CheckpointId, CommitCheckpoint},
            git_object::{ObjectFormat, ObjectId},
        };
        if !cfg!(target_os = "linux") {
            return Err(GitError::Unavailable(
                "commit checkpoint writes are not validated on this platform",
            ));
        }
        let ins = self.inspect(input)?;
        let hex = match ins.head {
            Head::Branch { commit, .. } | Head::Detached { commit } => commit,
            Head::Unborn { .. } => return Err(GitError::Parse("cannot checkpoint unborn HEAD")),
        };
        let format = ObjectFormat::parse(&ins.object_format)
            .ok_or(GitError::Parse("unsupported object format"))?;
        let commit = ObjectId::new(format, &hex).map_err(GitError::Parse)?;
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        if self.query(&cwd, &["cat-file", "-t", commit.as_hex()], "commit type")? != "commit" {
            return Err(GitError::Parse("HEAD is not a commit"));
        }
        let id = CheckpointId::new();
        let reference = format!("refs/rit/checkpoints/{id}/head");
        let zero = "0".repeat(commit.as_hex().len());
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            reference.as_str(),
            commit.as_hex(),
            zero.as_str(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(&cwd), &args, "commit-only checkpoint")?;
        Ok(CommitCheckpoint{id,reference,commit,capture_scope:"committed_head_only; index, working tree, untracked, ignored, submodule and LFS contents are not captured".into()})
    }
    /// Discover commit-only checkpoint references. This does not establish authority or integrity.
    pub fn checkpoint_list(
        &self,
        input: &Path,
    ) -> Result<Vec<rit_core::checkpoint::CommitCheckpoint>, GitError> {
        use rit_core::{
            checkpoint::{CheckpointId, CommitCheckpoint},
            git_object::ObjectId,
        };
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let format = self.object_format(&cwd)?;
        let text = self.query(
            &cwd,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(objecttype)",
                "refs/rit/checkpoints/",
            ],
            "checkpoint refs",
        )?;
        let mut result = Vec::new();
        for line in text.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            if fields.len() != 3 || fields[2] != "commit" {
                return Err(GitError::Parse("checkpoint ref fields"));
            }
            let id = fields[0]
                .strip_prefix("refs/rit/checkpoints/")
                .and_then(|s| s.strip_suffix("/head"))
                .ok_or(GitError::Parse("checkpoint ref name"))?
                .parse::<CheckpointId>()
                .map_err(|_| GitError::Parse("checkpoint ID"))?;
            result.push(CommitCheckpoint{id,reference:fields[0].into(),commit:ObjectId::new(format,fields[1]).map_err(GitError::Parse)?,capture_scope:"committed_head_only; index, working tree, untracked, ignored, submodule and LFS contents are not captured".into()});
        }
        Ok(result)
    }
    /// Check that a checkpoint's commit and reachable objects pass Git fsck, without a restore.
    pub fn checkpoint_verify(
        &self,
        input: &Path,
        id: rit_core::checkpoint::CheckpointId,
    ) -> Result<rit_core::checkpoint::CommitCheckpoint, GitError> {
        let item = self
            .checkpoint_list(input)?
            .into_iter()
            .find(|x| x.id == id)
            .ok_or(GitError::Parse("checkpoint not found"))?;
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        let args = [
            "-c",
            "core.hooksPath=/dev/null",
            "fsck",
            "--no-reflogs",
            "--no-progress",
            item.commit.as_hex(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        self.run(Some(&cwd), &args, "checkpoint integrity")?;
        Ok(item)
    }
    /// Inspect an existing repository without registering or modifying it.
    pub fn inspect(&self, input: &Path) -> Result<Inspection, GitError> {
        self.inspect_inner(input, 0)
    }
    fn inspect_inner(&self, input: &Path, depth: usize) -> Result<Inspection, GitError> {
        if depth > 4 {
            return Err(GitError::Unavailable("nested submodule depth exceeds four"));
        }
        let cwd = input.canonicalize().map_err(|source| GitError::Path {
            path: input.to_owned(),
            source,
        })?;
        if !cwd.is_dir() {
            return Err(GitError::Parse("repository path must be a directory"));
        }
        let bare = match self
            .query(
                &cwd,
                &["rev-parse", "--is-bare-repository"],
                "repository discovery",
            )?
            .as_str()
        {
            "true" => true,
            "false" => false,
            _ => return Err(GitError::Parse("bare flag")),
        };
        let path = |v: String| -> Result<String, GitError> {
            let p = PathBuf::from(v);
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            let p = p
                .canonicalize()
                .map_err(|source| GitError::Path { path: p, source })?;
            p.to_str().map(str::to_owned).ok_or(GitError::NonUnicode)
        };
        let git_dir =
            path(self.query(&cwd, &["rev-parse", "--absolute-git-dir"], "git directory")?)?;
        let common_git_dir = path(self.query(
            &cwd,
            &["rev-parse", "--git-common-dir"],
            "common Git directory",
        )?)?;
        let root = if bare {
            None
        } else {
            Some(path(self.query(
                &cwd,
                &["rev-parse", "--show-toplevel"],
                "working tree",
            )?)?)
        };
        let location = if bare {
            cwd.to_str().ok_or(GitError::NonUnicode)?.to_owned()
        } else {
            root.clone().ok_or(GitError::Parse("worktree root"))?
        };
        let format = self.query(
            &cwd,
            &["rev-parse", "--show-object-format"],
            "object format",
        )?;
        if format != "sha1" && format != "sha256" {
            return Err(GitError::Parse("unsupported object format"));
        }
        let shallow = match self
            .query(
                &cwd,
                &["rev-parse", "--is-shallow-repository"],
                "shallow state",
            )?
            .as_str()
        {
            "true" => true,
            "false" => false,
            _ => return Err(GitError::Parse("shallow state")),
        };
        let remotes = self
            .query(&cwd, &["remote"], "remote names")?
            .lines()
            .map(str::to_owned)
            .collect();
        let status = if bare {
            None
        } else {
            let args = [
                "status",
                "--ignore-submodules=dirty",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
            let mut status = crate::status::parse(&self.run(Some(&cwd), &args, "status")?)?;
            // Git ordinarily spawns another Git for dirty submodules; the restricted
            // process cannot do that. Inspect initialized submodules ourselves under
            // the same restriction and attach conservative dirty evidence.
            let entries = self.run(
                Some(&cwd),
                &[
                    OsString::from("ls-files"),
                    OsString::from("--stage"),
                    OsString::from("-z"),
                ],
                "index entries",
            )?;
            let tree = Path::new(root.as_deref().ok_or(GitError::Parse("worktree root"))?);
            let mut count = 0usize;
            for record in entries.split(|b| *b == 0).filter(|x| !x.is_empty()) {
                if !record.starts_with(b"160000 ") {
                    continue;
                }
                let tab = record
                    .iter()
                    .position(|b| *b == b'\t')
                    .ok_or(GitError::Parse("index entry"))?;
                if !record[..tab].ends_with(b" 0") {
                    continue;
                }
                count += 1;
                if count > 64 {
                    return Err(GitError::Unavailable("more than 64 submodules"));
                }
                let path =
                    std::str::from_utf8(&record[tab + 1..]).map_err(|_| GitError::NonUnicode)?;
                if Path::new(path)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(GitError::Parse("submodule path"));
                }
                let sub = tree.join(path);
                if !sub.join(".git").exists() {
                    continue;
                }
                let nested = self.inspect_inner(&sub, depth + 1)?;
                if nested.worktree.as_deref()
                    != sub
                        .canonicalize()
                        .ok()
                        .and_then(|x| x.to_str().map(str::to_owned))
                        .as_deref()
                {
                    return Err(GitError::Parse("submodule worktree mismatch"));
                }
                let Some(ns) = nested.status else { continue };
                let has_untracked = ns.changes.iter().any(|c| c.kind == "untracked");
                let has_modified = ns.changes.iter().any(|c| c.kind != "untracked");
                if !has_untracked && !has_modified {
                    continue;
                }
                if let Some(c) = status.changes.iter_mut().find(|c| c.path == path) {
                    let mut subflag = c
                        .submodule
                        .clone()
                        .unwrap_or_else(|| "S...".into())
                        .into_bytes();
                    if subflag.len() == 4 && subflag[0] == b'S' {
                        if has_modified {
                            subflag[2] = b'M'
                        }
                        if has_untracked {
                            subflag[3] = b'U'
                        }
                        c.submodule = String::from_utf8(subflag).ok();
                    }
                    if let Some(xy) = &mut c.xy
                        && xy.ends_with('.')
                    {
                        xy.replace_range(1..2, "M");
                    }
                } else {
                    status.changes.push(rit_core::repository::Change {
                        kind: "ordinary".into(),
                        path: path.into(),
                        original_path: None,
                        xy: Some(".M".into()),
                        submodule: Some(format!(
                            "S.{}{}",
                            if has_modified { 'M' } else { '.' },
                            if has_untracked { 'U' } else { '.' }
                        )),
                    });
                }
            }
            Some(status)
        };
        let head = if let Some(s) = &status {
            match (s.branch.as_deref(), s.oid.as_deref()) {
                (Some("(detached)"), Some(oid)) if oid != "(initial)" => {
                    Head::Detached { commit: oid.into() }
                }
                (Some(name), Some("(initial)")) => Head::Unborn { name: name.into() },
                (Some(name), Some(oid)) => Head::Branch {
                    name: name.into(),
                    commit: oid.into(),
                },
                _ => return Err(GitError::Parse("HEAD headers")),
            }
        } else {
            let name = self.query(&cwd, &["symbolic-ref", "--short", "HEAD"], "symbolic HEAD");
            let oid = self.query(&cwd, &["rev-parse", "--verify", "HEAD"], "HEAD commit");
            match (name, oid) {
                (Ok(name), Ok(commit)) => Head::Branch { name, commit },
                (Ok(name), Err(GitError::Exit { .. })) => Head::Unborn { name },
                (Err(GitError::Exit { .. }), Ok(commit)) => Head::Detached { commit },
                (Err(e), _) | (_, Err(e)) => return Err(e),
            }
        };
        Ok(Inspection {
            location,
            worktree: root,
            git_dir,
            common_git_dir,
            bare,
            object_format: format,
            shallow,
            head,
            remotes,
            status,
        })
    }
}

// Avoid blocking indefinitely in a cleanup wait; the reaper owns the child.
fn terminate(mut child: GroupChild) -> Result<(), GitError> {
    let result = child.kill();
    thread::spawn(move || {
        let _ = child.wait();
    });
    result.map_err(GitError::Termination)
}

// A narrowly scoped mitigation, not a general process/filesystem sandbox: block
// child process creation in Git's status/diff. Thread-style clone is allowed.
// clone3 returns ENOSYS so glibc can fall back to clone for legitimate threads.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn no_child_filter() -> Result<tempfile::NamedTempFile, GitError> {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    let tool = std::fs::metadata("/usr/bin/setpriv").map_err(|_| {
        GitError::Unavailable("restricted status requires trusted /usr/bin/setpriv")
    })?;
    if !tool.is_file() || tool.uid() != 0 || tool.mode() & 0o022 != 0 {
        return Err(GitError::Unavailable(
            "setpriv is not root-owned and protected",
        ));
    }
    // Linux seccomp BPF, x86-64: fork/vfork and non-thread clone => EPERM,
    // clone3 => ENOSYS; unexpected architectures are killed. Load arguments as
    // native-endian sock_filter records. No repo-controlled input enters program.
    let program: [(u16, u8, u8, u32); 15] = [
        (0x20, 0, 0, 4),
        (0x15, 1, 0, 0xc000003e),
        (0x06, 0, 0, 0x80000000),
        (0x20, 0, 0, 0),
        (0x15, 0, 1, 57),
        (0x06, 0, 0, 0x00050001),
        (0x15, 0, 1, 58),
        (0x06, 0, 0, 0x00050001),
        (0x15, 0, 1, 435),
        (0x06, 0, 0, 0x00050026),
        (0x15, 0, 3, 56),
        (0x20, 0, 0, 16),
        (0x45, 1, 0, 0x00010000),
        (0x06, 0, 0, 0x00050001),
        (0x06, 0, 0, 0x7fff0000),
    ];
    let mut f = tempfile::NamedTempFile::new().map_err(GitError::Launch)?;
    for (code, jt, jf, k) in program {
        f.write_all(&code.to_ne_bytes())
            .and_then(|_| f.write_all(&[jt, jf]))
            .and_then(|_| f.write_all(&k.to_ne_bytes()))
            .map_err(GitError::Launch)?;
    }
    f.flush().map_err(GitError::Launch)?;
    Ok(f)
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn no_child_filter() -> Result<tempfile::NamedTempFile, GitError> {
    Err(GitError::Unavailable(
        "restricted Git status/diff is not validated on this platform",
    ))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn workspace_target(dest: &Path) -> Result<PathBuf, GitError> {
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    if dest.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(GitError::Parse("parent traversal not allowed"));
    }
    let absolute = if dest.is_absolute() {
        dest.to_owned()
    } else {
        std::env::current_dir()
            .map_err(GitError::Launch)?
            .join(dest)
    };
    let leaf = absolute
        .file_name()
        .ok_or(GitError::Parse("workspace path must have a name"))?;
    if leaf == ".git" {
        return Err(GitError::Parse("invalid workspace path"));
    }
    let parent = absolute
        .parent()
        .ok_or(GitError::Parse("workspace parent"))?;
    let uid = rustix::process::geteuid().as_raw();
    let mut current = PathBuf::from("/");
    for component in parent.components() {
        if let Component::Normal(n) = component {
            current.push(n);
            let st = std::fs::symlink_metadata(&current).map_err(|source| GitError::Path {
                path: current.clone(),
                source,
            })?;
            if !st.is_dir()
                || st.file_type().is_symlink()
                || (st.uid() != 0 && st.uid() != uid)
                || (st.mode() & 0o022 != 0 && st.mode() & 0o1000 == 0)
            {
                return Err(GitError::Unavailable(
                    "workspace ancestry must be trusted and non-symlinked",
                ));
            }
        }
    }
    let st = std::fs::metadata(parent).map_err(GitError::Launch)?;
    if st.uid() != uid || st.mode() & 0o077 != 0 {
        return Err(GitError::Unavailable("workspace parent must be owner-only"));
    }
    let target = parent.canonicalize().map_err(GitError::Launch)?.join(leaf);
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err(GitError::Unavailable("workspace path already exists"));
    }
    Ok(target)
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn workspace_target(_: &Path) -> Result<PathBuf, GitError> {
    Err(GitError::Unavailable(
        "workspace paths not supported on this platform",
    ))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn materialize_files(
    root: &Path,
    files: Vec<(String, String, bool, Vec<u8>)>,
) -> Result<(), GitError> {
    use rustix::fs::{Mode, OFlags, mkdirat, open, openat};
    use std::{io::Write, os::unix::fs::PermissionsExt, path::Component};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    for (path, _, executable, data) in files {
        let mut dir = open(root, flags, Mode::empty()).map_err(|e| GitError::Launch(e.into()))?;
        let mut components = Path::new(&path).components().peekable();
        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return Err(GitError::Parse("invalid tree path"));
            };
            if components.peek().is_none() {
                let fd = openat(
                    &dir,
                    name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(if executable { 0o755 } else { 0o644 }),
                )
                .map_err(|e| GitError::Launch(e.into()))?;
                let mut file = std::fs::File::from(fd);
                file.write_all(&data).map_err(GitError::Launch)?;
                file.set_permissions(std::fs::Permissions::from_mode(if executable {
                    0o755
                } else {
                    0o644
                }))
                .map_err(GitError::Launch)?;
            } else {
                match mkdirat(&dir, name, Mode::from_raw_mode(0o700)) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                    Err(e) => return Err(GitError::Launch(e.into())),
                }
                dir = openat(&dir, name, flags, Mode::empty())
                    .map_err(|e| GitError::Launch(e.into()))?;
            }
        }
    }
    Ok(())
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn materialize_files(_: &Path, _: Vec<(String, String, bool, Vec<u8>)>) -> Result<(), GitError> {
    Err(GitError::Unavailable(
        "workspace materialization unsupported on this platform",
    ))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_worktree_file(root: &Path, path: &str) -> Result<Option<Vec<u8>>, GitError> {
    use rustix::fs::{Mode, OFlags, fstat, open, openat};
    use std::io::Read;
    use std::path::Component;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir = open(root, flags, Mode::empty()).map_err(|e| GitError::Launch(e.into()))?;
    let mut parts = Path::new(path).components().peekable();
    while let Some(Component::Normal(n)) = parts.next() {
        if parts.peek().is_some() {
            dir = match openat(&dir, n, flags, Mode::empty()) {
                Ok(f) => f,
                Err(
                    rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP,
                ) => return Ok(None),
                Err(e) => return Err(GitError::Launch(e.into())),
            };
            continue;
        }
        let fd = match openat(
            &dir,
            n,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(f) => f,
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                return Ok(None);
            }
            Err(e) => return Err(GitError::Launch(e.into())),
        };
        let before = fstat(&fd).map_err(|e| GitError::Launch(e.into()))?;
        if before.st_mode & 0o170000 != 0o100000 {
            return Ok(None);
        }
        if before.st_size < 0 || before.st_size > 1_048_576 {
            return Err(GitError::Parse("worktree source exceeds 1 MiB"));
        }
        let file = std::fs::File::from(fd);
        let mut bytes = Vec::new();
        (&file)
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(GitError::Launch)?;
        let after = fstat(&file).map_err(|e| GitError::Launch(e.into()))?;
        if bytes.len() > 1_048_576
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            return Err(GitError::Unavailable("worktree source changed while read"));
        }
        return Ok(Some(bytes));
    }
    Err(GitError::Parse("worktree source path"))
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn read_worktree_file(_: &Path, _: &str) -> Result<Option<Vec<u8>>, GitError> {
    Err(GitError::Unavailable(
        "worktree source reads unsupported on this platform",
    ))
}

fn validate_checkpoint_path(path: &str) -> Result<(), GitError> {
    if path.is_empty()
        || path.len() > 4096
        || Path::new(path).components().any(|c| match c {
            std::path::Component::Normal(n) => n.to_string_lossy().eq_ignore_ascii_case(".git"),
            _ => true,
        })
    {
        return Err(GitError::Parse("invalid checkpoint path"));
    }
    Ok(())
}
type CapturedRawEntry = Option<(&'static str, bool, Vec<u8>)>;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_checkpoint_entry(
    root: &Path,
    path: &str,
    limit: usize,
) -> Result<CapturedRawEntry, GitError> {
    use rustix::fs::{AtFlags, Mode, OFlags, fstat, open, openat, readlinkat, statat};
    use std::io::Read;
    use std::path::Component;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir = open(root, flags, Mode::empty()).map_err(|e| GitError::Launch(e.into()))?;
    let mut parts = Path::new(path).components().peekable();
    while let Some(Component::Normal(name)) = parts.next() {
        if parts.peek().is_some() {
            dir = match openat(&dir, name, flags, Mode::empty()) {
                Ok(fd) => fd,
                Err(
                    rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP,
                ) => return Ok(None),
                Err(e) => return Err(GitError::Launch(e.into())),
            };
            continue;
        }
        let st = match statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(st) => st,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(e) => return Err(GitError::Launch(e.into())),
        };
        if st.st_mode & 0o170000 == 0o120000 {
            let target =
                readlinkat(&dir, name, Vec::new()).map_err(|e| GitError::Launch(e.into()))?;
            let after = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|e| GitError::Launch(e.into()))?;
            if st.st_ino != after.st_ino
                || st.st_dev != after.st_dev
                || st.st_ctime != after.st_ctime
                || st.st_ctime_nsec != after.st_ctime_nsec
            {
                return Err(GitError::Unavailable("symlink changed during checkpoint"));
            }
            let bytes = target.into_bytes();
            if bytes.len() > limit {
                return Err(GitError::OutputLimit);
            }
            return Ok(Some(("symlink", false, bytes)));
        }
        if st.st_mode & 0o170000 != 0o100000 {
            return Err(GitError::Unavailable(
                "unsupported non-regular checkpoint path",
            ));
        }
        let fd = openat(
            &dir,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|e| GitError::Launch(e.into()))?;
        let before = fstat(&fd).map_err(|e| GitError::Launch(e.into()))?;
        if before.st_mode & 0o170000 != 0o100000
            || before.st_size < 0
            || before.st_size as u64 > limit as u64
        {
            return Err(GitError::Unavailable(
                "checkpoint file unsupported or exceeds selected file budget",
            ));
        }
        let file = std::fs::File::from(fd);
        let mut bytes = Vec::new();
        (&file)
            .take((limit as u64) + 1)
            .read_to_end(&mut bytes)
            .map_err(GitError::Launch)?;
        let after = fstat(&file).map_err(|e| GitError::Launch(e.into()))?;
        if bytes.len() > limit
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            return Err(GitError::Unavailable("checkpoint file changed during read"));
        }
        return Ok(Some(("regular", before.st_mode & 0o111 != 0, bytes)));
    }
    Err(GitError::Parse("checkpoint path"))
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn read_checkpoint_entry(_: &Path, _: &str, _: usize) -> Result<CapturedRawEntry, GitError> {
    Err(GitError::Unavailable(
        "content checkpoint unsupported on this platform",
    ))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn restore_create_entry(
    root: &Path,
    path: &str,
    kind: &str,
    executable: bool,
    bytes: &[u8],
) -> Result<(), GitError> {
    use rustix::fs::{Mode, OFlags, mkdirat, open, openat, symlinkat};
    use std::{
        io::Write,
        os::unix::{ffi::OsStrExt, fs::PermissionsExt},
        path::Component,
    };
    validate_checkpoint_path(path)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir = open(root, flags, Mode::empty()).map_err(|e| GitError::Launch(e.into()))?;
    let mut parts = Path::new(path).components().peekable();
    while let Some(Component::Normal(name)) = parts.next() {
        if parts.peek().is_some() {
            match mkdirat(&dir, name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                Err(e) => return Err(GitError::Launch(e.into())),
            };
            dir =
                openat(&dir, name, flags, Mode::empty()).map_err(|e| GitError::Launch(e.into()))?;
            continue;
        }
        if kind == "symlink" {
            symlinkat(OsStr::from_bytes(bytes), &dir, name)
                .map_err(|e| GitError::Launch(e.into()))?;
        } else if kind == "regular" {
            let fd = openat(
                &dir,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(if executable { 0o755 } else { 0o644 }),
            )
            .map_err(|e| GitError::Launch(e.into()))?;
            let mut f = std::fs::File::from(fd);
            f.write_all(bytes).map_err(GitError::Launch)?;
            f.set_permissions(std::fs::Permissions::from_mode(if executable {
                0o755
            } else {
                0o644
            }))
            .map_err(GitError::Launch)?;
            f.sync_all().map_err(GitError::Launch)?;
        } else {
            return Err(GitError::Parse("unsupported restore kind"));
        }
        return Ok(());
    }
    Err(GitError::Parse("restore path"))
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn restore_create_entry(_: &Path, _: &str, _: &str, _: bool, _: &[u8]) -> Result<(), GitError> {
    Err(GitError::Unavailable("restore unsupported"))
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn restore_create_index(gitdir: &Path, bytes: &[u8]) -> Result<(), GitError> {
    use rustix::fs::{Mode, OFlags, RenameFlags, open, openat, renameat_with};
    use std::io::Write;
    let dir = open(
        gitdir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| GitError::Launch(e.into()))?;
    let fd = openat(
        &dir,
        "index.lock",
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|e| GitError::Launch(e.into()))?;
    let mut file = std::fs::File::from(fd);
    file.write_all(bytes).map_err(GitError::Launch)?;
    file.sync_all().map_err(GitError::Launch)?;
    drop(file);
    renameat_with(&dir, "index.lock", &dir, "index", RenameFlags::NOREPLACE)
        .map_err(|e| GitError::Launch(e.into()))?;
    Ok(())
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn restore_create_index(_: &Path, _: &[u8]) -> Result<(), GitError> {
    Err(GitError::Unavailable("restore unsupported"))
}

fn parse_lfs_pointer(bytes: &[u8]) -> Option<(String, u64)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let mut oid = None;
    let mut size = None;
    for line in lines {
        if let Some(s) = line.strip_prefix("oid sha256:") {
            if oid.is_some()
                || s.len() != 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return None;
            }
            oid = Some(s.to_owned());
        } else if let Some(s) = line.strip_prefix("size ") {
            if size.is_some() || s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            size = s.parse::<u64>().ok();
        } else if !line.starts_with("ext-") {
            return None;
        }
    }
    Some((oid?, size?))
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_default_lfs_payload(common: &Path, hash: &str, expected: u64) -> String {
    use rustix::fs::{Mode, OFlags, fstat, open, openat};
    use sha2::{Digest, Sha256};
    use std::io::Read;
    if expected > 64 * 1024 * 1024 {
        return "not_checked_size_limit".into();
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let Ok(mut dir) = open(common, flags, Mode::empty()) else {
        return "unavailable_or_unsafe_path".into();
    };
    for component in ["lfs", "objects", &hash[..2], &hash[2..4]] {
        dir = match openat(&dir, component, flags, Mode::empty()) {
            Ok(v) => v,
            Err(rustix::io::Errno::NOENT) => return "missing".into(),
            Err(_) => return "unavailable_or_unsafe_path".into(),
        };
    }
    let fd = match openat(
        &dir,
        hash,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(v) => v,
        Err(rustix::io::Errno::NOENT) => return "missing".into(),
        Err(_) => return "unavailable_or_unsafe_path".into(),
    };
    let Ok(before) = fstat(&fd) else {
        return "unavailable".into();
    };
    if before.st_mode & 0o170000 != 0o100000 || before.st_size < 0 {
        return "unsupported_file_type".into();
    }
    if before.st_size as u64 != expected {
        return "size_mismatch".into();
    }
    let mut file = std::fs::File::from(fd);
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                total += n as u64;
                if total > expected {
                    return "changed_during_read".into();
                }
                digest.update(&buf[..n]);
            }
            Err(_) => return "unavailable".into(),
        }
    }
    let Ok(after) = fstat(&file) else {
        return "unavailable".into();
    };
    if total != expected
        || before.st_size != after.st_size
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
        || before.st_ctime != after.st_ctime
        || before.st_ctime_nsec != after.st_ctime_nsec
    {
        return "changed_during_read".into();
    }
    let actual = format!("{:x}", digest.finalize());
    if actual == hash {
        "available_and_sha256_verified".into()
    } else {
        "sha256_mismatch".into()
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn check_default_lfs_payload(_: &Path, _: &str, _: u64) -> String {
    "platform_not_checked".into()
}
