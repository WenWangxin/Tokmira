//! Watches the local logs and the app's own records, rescans what changed, and tells
//! the window, so pages update without the refresh button. Nothing here touches the
//! network; official quota queries stay behind the user's switches.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use serde::Serialize;

/// Quiet time after the last change before rescanning.
const QUIET: Duration = Duration::from_millis(1500);
/// Upper bound while a session keeps writing, so updates still arrive every few seconds.
const MAX_WAIT: Duration = Duration::from_secs(4);
/// How often to retry watching directories that did not exist yet.
const RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    ClaudeLogs,
    CodexLogs,
    ClaudeRecords,
    CodexRecords,
}

/// Which tool's data a changed path belongs to; `None` for anything else.
pub fn classify(path: &Path, home: &Path, data_dir: &Path) -> Option<Change> {
    if path.starts_with(home.join(".claude/projects")) {
        return Some(Change::ClaudeLogs);
    }
    let codex = home.join(".codex");
    if path.starts_with(codex.join("sessions"))
        || path.starts_with(codex.join("archived_sessions"))
        || path == codex.join("session_index.jsonl")
    {
        return Some(Change::CodexLogs);
    }
    let name = path.file_name()?.to_str()?;
    if path.parent() == Some(&data_dir.join("recorder"))
        && matches!(name, "statusline.jsonl" | "hooks.jsonl")
    {
        return Some(Change::ClaudeRecords);
    }
    if path.parent() == Some(&data_dir.join("quota")) {
        return match name {
            "claude.jsonl" => Some(Change::ClaudeRecords),
            "codex.jsonl" | "codex-daily.json" => Some(Change::CodexRecords),
            _ => None,
        };
    }
    None
}

/// Coalesces bursts of file events: fires after `quiet` without new events, or
/// `max_wait` after the first one, whichever comes first.
#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    max_wait: Duration,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debouncer {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            quiet,
            max_wait,
            first: None,
            last: None,
        }
    }

    pub fn event(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// Time until the pending batch is due, or `None` when nothing is pending.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        let (first, last) = (self.first?, self.last?);
        let due = (last + self.quiet).min(first + self.max_wait);
        Some(due.saturating_duration_since(now))
    }

    /// Takes the pending batch if it is due.
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.wait(now).is_some_and(|wait| wait.is_zero()) {
            self.first = None;
            self.last = None;
            true
        } else {
            false
        }
    }
}

/// Payload of the `usage-changed` event: which tools changed. Never any content.
#[derive(Clone, Debug, Serialize)]
pub struct UsageChanged {
    pub tools: Vec<&'static str>,
}

/// What the watcher needs from the app: rescan the logs (returning which tools'
/// scans changed) and deliver the event to the window.
pub trait Sink: Send + 'static {
    fn rescan(&self) -> (bool, bool);
    fn notify(&self, change: UsageChanged);
}

/// The path with symlinks resolved; for a folder that does not exist yet, its
/// nearest existing parent is resolved and the rest appended.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolved(parent).join(name),
        _ => path.to_path_buf(),
    }
}

fn watch_targets(home: &Path, data_dir: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    vec![
        (home.join(".claude/projects"), RecursiveMode::Recursive),
        (home.join(".codex/sessions"), RecursiveMode::Recursive),
        (
            home.join(".codex/archived_sessions"),
            RecursiveMode::Recursive,
        ),
        (home.join(".codex"), RecursiveMode::NonRecursive),
        (data_dir.join("recorder"), RecursiveMode::NonRecursive),
        (data_dir.join("quota"), RecursiveMode::NonRecursive),
    ]
}

fn watch_available(
    watcher: &mut dyn Watcher,
    targets: &[(PathBuf, RecursiveMode)],
    watched: &mut BTreeSet<PathBuf>,
) -> bool {
    for (path, mode) in targets {
        if !watched.contains(path) && path.is_dir() {
            if watcher.watch(path, *mode).is_err() {
                return false;
            }
            watched.insert(path.clone());
        }
    }
    true
}

/// Fallback when native file events are unavailable (e.g. in a sandbox). Each pass
/// compares metadata of every log file, so it runs every few seconds, not continuously.
fn poll_watcher(sender: mpsc::Sender<notify::Result<notify::Event>>) -> Option<Box<dyn Watcher>> {
    notify::PollWatcher::new(
        sender,
        notify::Config::default().with_poll_interval(Duration::from_secs(5)),
    )
    .ok()
    .map(|watcher| Box::new(watcher) as Box<dyn Watcher>)
}

/// Starts the watcher thread. Failing to create a watcher only disables live
/// updates; the refresh button keeps working.
pub fn spawn(home: PathBuf, data_dir: PathBuf, sink: impl Sink) {
    std::thread::spawn(move || {
        let (sender, receiver) = mpsc::channel();
        let (mut watcher, mut polling): (Box<dyn Watcher>, bool) =
            match notify::recommended_watcher(sender.clone()) {
                Ok(watcher) => (Box::new(watcher), false),
                Err(_) => match poll_watcher(sender.clone()) {
                    Some(watcher) => (watcher, true),
                    None => return,
                },
            };
        // File events carry resolved paths (on macOS `/var` is really `/private/var`,
        // and `~/.claude` may be a symlink), so match against both spellings.
        let real_home = resolved(&home);
        let real_data_dir = resolved(&data_dir);
        let targets = watch_targets(&home, &data_dir);
        let mut watched = BTreeSet::new();
        if !watch_available(&mut *watcher, &targets, &mut watched) {
            let Some(replacement) = poll_watcher(sender.clone()) else {
                return;
            };
            watcher = replacement;
            polling = true;
            watched.clear();
            if !watch_available(&mut *watcher, &targets, &mut watched) {
                return;
            }
        }
        let mut last_retry = Instant::now();
        let mut debouncer = Debouncer::new(QUIET, MAX_WAIT);
        let mut pending = BTreeSet::new();
        loop {
            let now = Instant::now();
            let timeout = debouncer.wait(now).unwrap_or(RETRY).min(RETRY);
            match receiver.recv_timeout(timeout) {
                Ok(Ok(event)) => {
                    let changes: Vec<_> = event
                        .paths
                        .iter()
                        .filter_map(|path| {
                            classify(path, &home, &data_dir)
                                .or_else(|| classify(path, &real_home, &real_data_dir))
                        })
                        .collect();
                    if !changes.is_empty() {
                        pending.extend(changes);
                        debouncer.event(Instant::now());
                    }
                }
                Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if last_retry.elapsed() >= RETRY {
                if !watch_available(&mut *watcher, &targets, &mut watched) && !polling {
                    if let Some(replacement) = poll_watcher(sender.clone()) {
                        watcher = replacement;
                        polling = true;
                        watched.clear();
                        let _ = watch_available(&mut *watcher, &targets, &mut watched);
                    }
                }
                last_retry = Instant::now();
            }
            if !debouncer.take_due(Instant::now()) {
                continue;
            }
            let batch = std::mem::take(&mut pending);
            let logs = batch.contains(&Change::ClaudeLogs) || batch.contains(&Change::CodexLogs);
            let (claude_scan, codex_scan) = if logs { sink.rescan() } else { (false, false) };
            let mut tools = Vec::new();
            if claude_scan || batch.contains(&Change::ClaudeRecords) {
                tools.push("Claude");
            }
            if codex_scan || batch.contains(&Change::CodexRecords) {
                tools.push("Codex");
            }
            if !tools.is_empty() {
                sink.notify(UsageChanged { tools });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debouncer_waits_for_quiet_but_not_forever() {
        let start = Instant::now();
        let mut debouncer = Debouncer::new(Duration::from_millis(1500), Duration::from_secs(4));
        assert_eq!(debouncer.wait(start), None);
        assert!(!debouncer.take_due(start));

        debouncer.event(start);
        assert_eq!(debouncer.wait(start), Some(Duration::from_millis(1500)));
        assert!(!debouncer.take_due(start + Duration::from_millis(1499)));

        // A steady stream of writes keeps pushing the quiet deadline out...
        for step in 1..=3 {
            debouncer.event(start + Duration::from_secs(step));
        }
        assert!(!debouncer.take_due(start + Duration::from_millis(3900)));
        // ...but the batch is still due four seconds after the first change.
        assert!(debouncer.take_due(start + Duration::from_secs(4)));
        assert_eq!(debouncer.wait(start + Duration::from_secs(4)), None);

        // After a quiet gap a new batch starts fresh.
        let later = start + Duration::from_secs(10);
        debouncer.event(later);
        assert!(debouncer.take_due(later + Duration::from_millis(1500)));
    }

    #[test]
    fn watcher_reports_a_new_log_line() {
        use std::sync::{Arc, Mutex};

        struct Recorder(Arc<Mutex<Vec<Vec<&'static str>>>>);
        impl Sink for Recorder {
            fn rescan(&self) -> (bool, bool) {
                (true, false)
            }
            fn notify(&self, change: UsageChanged) {
                self.0.lock().expect("record").push(change.tools);
            }
        }

        let root = std::env::temp_dir().join(format!("token-audit-watch-{}", std::process::id()));
        let home = root.join("home");
        let data = root.join("data");
        let project = home.join(".claude/projects/demo");
        std::fs::create_dir_all(&project).expect("create projects");
        let seen = Arc::new(Mutex::new(Vec::new()));
        spawn(home.clone(), data, Recorder(Arc::clone(&seen)));
        // Give the watcher a moment to register before writing.
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(project.join("session.jsonl"), "{}\n").expect("write log");

        let deadline = Instant::now() + Duration::from_secs(10);
        while seen.lock().expect("read").is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(seen.lock().expect("read").first(), Some(&vec!["Claude"]));
    }

    #[test]
    fn classify_maps_paths_to_tools() {
        let home = Path::new("/home/me");
        let data = Path::new("/data/token-audit");
        let cases = [
            (
                "/home/me/.claude/projects/p/s.jsonl",
                Some(Change::ClaudeLogs),
            ),
            (
                "/home/me/.claude/projects/p/s/subagents/a.jsonl",
                Some(Change::ClaudeLogs),
            ),
            ("/home/me/.claude/settings.json", None),
            (
                "/home/me/.codex/sessions/2026/10/25/rollout-x.jsonl",
                Some(Change::CodexLogs),
            ),
            (
                "/home/me/.codex/archived_sessions/rollout-y.jsonl",
                Some(Change::CodexLogs),
            ),
            (
                "/home/me/.codex/session_index.jsonl",
                Some(Change::CodexLogs),
            ),
            ("/home/me/.codex/config.toml", None),
            (
                "/data/token-audit/recorder/statusline.jsonl",
                Some(Change::ClaudeRecords),
            ),
            ("/data/token-audit/recorder/state.json", None),
            (
                "/data/token-audit/quota/claude.jsonl",
                Some(Change::ClaudeRecords),
            ),
            (
                "/data/token-audit/quota/codex.jsonl",
                Some(Change::CodexRecords),
            ),
            (
                "/data/token-audit/quota/codex-daily.json",
                Some(Change::CodexRecords),
            ),
            ("/data/token-audit/settings.json", None),
        ];
        for (path, expected) in cases {
            assert_eq!(classify(Path::new(path), home, data), expected, "{path}");
        }
    }
}
