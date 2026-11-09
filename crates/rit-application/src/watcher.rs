//! Bounded, ephemeral filesystem notifications with Git worktree reconciliation.
//! Notifications can be lost; this is not a durable audit or an isolation boundary.
use crate::{GitReadService, WorkspaceService};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};
use rit_git::Git;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// One normalized filesystem notification. Actor and task are intentionally unknown.
#[derive(Debug, Serialize)]
pub struct ActivityEvent {
    /// Opaque event identifier, not a durable audit record.
    pub event_id: String,
    /// Normalized filesystem category.
    pub event_type: &'static str,
    /// Native notifications do not provide a trustworthy occurrence time.
    pub occurred_at: Option<u64>,
    /// Observation time in Unix milliseconds.
    pub observed_at: u128,
    /// Unknown without authenticated repository registration.
    pub repository_id: Option<String>,
    /// Manifest-derived coordination ID, not authenticated.
    pub workspace_id: Option<String>,
    /// Not inferred from OS process or branch.
    pub actor_id: Option<String>,
    /// Unknown without durable task linkage.
    pub task_id: Option<String>,
    /// Unknown without durable operation linkage.
    pub operation_id: Option<String>,
    /// Source of evidence.
    pub source: &'static str,
    /// Git-worktree-relative path.
    pub path: String,
    /// Old path only when native watcher reports both sides together.
    pub old_path: Option<String>,
    /// Native event category, not a semantic conflict.
    pub change_kind: &'static str,
    /// Notification evidence does not imply a complete event history.
    pub confidence: &'static str,
    /// No inferred correlation.
    pub correlation_id: Option<String>,
}
/// One bounded batch of filesystem notifications.
#[derive(Serialize)]
pub struct WatchBatch {
    /// Normalized, deduplicated events.
    pub events: Vec<ActivityEvent>,
    /// True when queue overflow or native rescan was reported; reconcile Git.
    pub overflowed: bool,
    /// True when native watcher reported errors; restart and reconcile.
    pub watcher_error: bool,
    /// Exact duplicate notifications suppressed in the debounce window.
    pub duplicates_suppressed: usize,
}
type Signal = notify::Result<Event>;
/// Live process-local watcher. Dropping it closes the native watcher and buffer.
pub struct ActivityWatcher {
    git: Git,
    repo: PathBuf,
    watcher: RecommendedWatcher,
    capacity: usize,
    rx: Receiver<Signal>,
    overflow: Arc<AtomicBool>,
    roots: BTreeMap<PathBuf, Option<String>>,
    recent: HashMap<(String, String, String), Instant>,
}
fn make_watcher(
    tx: SyncSender<Signal>,
    overflow: Arc<AtomicBool>,
) -> Result<RecommendedWatcher, String> {
    RecommendedWatcher::new(
        move |event| match tx.try_send(event) {
            Ok(()) => (),
            Err(TrySendError::Full(_)) => overflow.store(true, Ordering::Relaxed),
            Err(TrySendError::Disconnected(_)) => (),
        },
        Config::default().with_follow_symlinks(false),
    )
    .map_err(|e| e.to_string())
}
impl ActivityWatcher {
    /// Start with a bounded native notification queue, then register current worktrees.
    pub fn start(git: Git, repo: &Path, capacity: usize) -> Result<Self, String> {
        if !(1..=4096).contains(&capacity) {
            return Err("watcher capacity must be 1..4096".into());
        }
        let repo = repo.canonicalize().map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::sync_channel(capacity);
        let overflow = Arc::new(AtomicBool::new(false));
        let watcher = make_watcher(tx.clone(), overflow.clone())?;
        let mut s = Self {
            git,
            repo,
            watcher,
            capacity,
            rx,
            overflow,
            roots: BTreeMap::new(),
            recent: HashMap::new(),
        };
        s.reconcile_watches()?;
        Ok(s)
    }
    /// Refresh watch roots from actual Git inventory and RIT branch manifests.
    pub fn reconcile_watches(&mut self) -> Result<(), String> {
        let read = GitReadService::new(self.git.clone());
        let inventory = read.worktrees(&self.repo).map_err(|e| e.to_string())?;
        if inventory.len() > 64 {
            return Err("more than 64 worktrees".into());
        }
        let managed = WorkspaceService::new(self.git.clone())
            .reconcile(&self.repo)
            .map_err(|e| e.to_string())?;
        let mut roots = BTreeMap::new();
        for w in inventory {
            if w.bare || w.prunable {
                continue;
            }
            let path = PathBuf::from(&w.path);
            if !path.is_dir() {
                continue;
            }
            let path = path.canonicalize().unwrap_or(path);
            let id = managed
                .workspaces
                .iter()
                .find(|m| {
                    m.worktree
                        .as_ref()
                        .is_some_and(|actual| actual.path == w.path)
                        && m.observation == "present"
                })
                .map(|m| m.manifest.id.to_string());
            roots.insert(path, id);
        }
        let stale = self
            .roots
            .keys()
            .filter(|p| !roots.contains_key(*p))
            .cloned()
            .collect::<Vec<_>>();
        for old in stale {
            let _ = self.watcher.unwatch(&old);
        }
        let fresh = roots
            .keys()
            .filter(|p| !self.roots.contains_key(*p))
            .cloned()
            .collect::<Vec<_>>();
        for new in fresh {
            self.watcher
                .watch(&new, RecursiveMode::Recursive)
                .map_err(|e| e.to_string())?;
        }
        self.roots = roots;
        Ok(())
    }
    /// Restart the watcher and re-register actual worktrees; missed events cannot be reconstructed.
    pub fn restart(&mut self) -> Result<(), String> {
        // A previous generation may have filled the bounded channel. Reusing it can
        // drop the first new notification even after native watches are reinstalled.
        // The old generation is intentionally discarded and its loss is explicit.
        let (tx, rx) = mpsc::sync_channel(self.capacity);
        let overflow = Arc::new(AtomicBool::new(true));
        let watcher = make_watcher(tx, overflow.clone())?;
        self.watcher = watcher;
        self.rx = rx;
        self.overflow = overflow;
        self.roots.clear();
        self.recent.clear();
        self.reconcile_watches()
    }
    /// Wait for and normalize a bounded batch. Overflow means the caller must reconcile.
    pub fn poll(&mut self, timeout: Duration) -> WatchBatch {
        let mut incoming = Vec::new();
        if let Ok(e) = self.rx.recv_timeout(timeout) {
            incoming.push(e)
        }
        while incoming.len() < 4096 {
            match self.rx.try_recv() {
                Ok(e) => incoming.push(e),
                Err(_) => break,
            }
        }
        let mut overflowed = self.overflow.swap(false, Ordering::Relaxed);
        let mut watcher_error = false;
        let mut events = Vec::new();
        let mut duplicates = 0;
        let now = Instant::now();
        self.recent
            .retain(|_, t| now.duration_since(*t) < Duration::from_millis(150));
        for item in incoming {
            let e = match item {
                Ok(e) => e,
                Err(_) => {
                    watcher_error = true;
                    continue;
                }
            };
            if e.need_rescan() {
                overflowed = true;
            }
            let kind = match e.kind {
                EventKind::Create(_) => "file_created",
                EventKind::Remove(_) => "file_deleted",
                EventKind::Modify(ModifyKind::Name(_)) => "file_renamed",
                EventKind::Modify(ModifyKind::Metadata(_)) => "metadata_changed",
                EventKind::Modify(_) => "file_modified",
                _ => continue,
            };
            let is_both = matches!(
                e.kind,
                EventKind::Modify(ModifyKind::Name(RenameMode::Both))
            ) && e.paths.len() == 2;
            let old = if is_both { e.paths.first() } else { None };
            let chosen = if is_both {
                e.paths.iter().skip(1).collect::<Vec<_>>()
            } else {
                e.paths.iter().collect::<Vec<_>>()
            };
            for path in chosen {
                let Some((root, id)) = self
                    .roots
                    .iter()
                    .filter(|(r, _)| path.starts_with(r))
                    .max_by_key(|(r, _)| r.components().count())
                else {
                    continue;
                };
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                if relative
                    .components()
                    .any(|c| matches!(c,Component::Normal(n) if n==".git"))
                {
                    continue;
                }
                let Some(path) = relative.to_str().filter(|s| !s.is_empty()) else {
                    overflowed = true;
                    continue;
                };
                let old_path = old
                    .and_then(|v| v.strip_prefix(root).ok())
                    .and_then(Path::to_str)
                    .map(str::to_owned);
                let key = (
                    root.to_string_lossy().into_owned(),
                    path.to_owned(),
                    format!("{kind}:{old_path:?}"),
                );
                if self.recent.contains_key(&key) {
                    duplicates += 1;
                    continue;
                }
                if self.recent.len() >= 4096 {
                    overflowed = true;
                    self.recent.clear()
                }
                self.recent.insert(key, now);
                let observed_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |v| v.as_millis());
                events.push(ActivityEvent {
                    event_id: rit_core::workspace::WorkspaceId::new().to_string(),
                    event_type: kind,
                    occurred_at: None,
                    observed_at,
                    repository_id: None,
                    workspace_id: id.clone(),
                    actor_id: None,
                    task_id: None,
                    operation_id: None,
                    source: "filesystem_notification",
                    path: path.into(),
                    old_path,
                    change_kind: kind,
                    confidence: "observed_notification_incomplete_history",
                    correlation_id: None,
                });
            }
        }
        WatchBatch {
            events,
            overflowed,
            watcher_error,
            duplicates_suppressed: duplicates,
        }
    }
}
