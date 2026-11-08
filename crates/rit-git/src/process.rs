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
    #[error("workspace {id} provisioning interrupted; reconcile before retrying")]
    Provisioning { id: String },
    /// Git executable was unavailable or could not be spawned.
    #[error("cannot start Git executable: {0}")]
    Launch(#[source] io::Error),
    /// Git version could not be parsed.
    #[error("unrecognized Git version output")]
    Version,
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
        // Status and worktree diffs may invoke clean filters, even with --no-ext-diff.
        // Fail closed unless the Linux x86-64 child-process restriction can be installed.
        let restricted =
            matches!(operation, "status" | "diff") || operation.starts_with("workspace ");
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = cwd {
            cmd.current_dir(path);
        }
        let mut child = cmd.group_spawn().map_err(GitError::Launch)?;
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
        let status = status.ok_or(GitError::Parse("process status"))?;
        if !status.success() {
            return Err(GitError::Exit {
                operation,
                code: status.code(),
            });
        }
        outputs[0]
            .take()
            .ok_or(GitError::Parse("stdout"))?
            .map_err(|_| GitError::Parse("stdout read"))
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
        use rit_core::git_object::{ObjectId, SourceFile, SourceSnapshot};
        let ins = self.inspect(input)?;
        let commit = match ins.head {
            Head::Branch { commit, .. } | Head::Detached { commit } => commit,
            Head::Unborn { .. } => {
                return Err(GitError::Parse("unborn repository has no source snapshot"));
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
            if !(path.ends_with(".py") || path.ends_with(".ts") || path.ends_with(".go")) {
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
            commit,
            files,
            omitted_non_regular: omitted,
        })
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
            workspace::{WorkspaceId, WorkspaceManifest},
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
        let old = self.workspace_write_manifest(cwd, &manifest, &zero)?;
        let action = (|| -> Result<(), GitError> {
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
            Ok(())
        })();
        if action.is_err() {
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
            let observation = match &w {
                None => "missing",
                Some(w) if w.path != m.path => "moved",
                Some(w) if w.prunable => "prunable",
                Some(_) if m.state == "provisioning" => "interrupted",
                Some(_) if m.state == "no_checkout" => "no_checkout",
                Some(_) => "present",
            }
            .to_string();
            let status = if observation == "present" {
                self.inspect(Path::new(&m.path)).ok().and_then(|i| i.status)
            } else {
                None
            };
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
