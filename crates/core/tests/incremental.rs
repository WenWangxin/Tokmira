//! The incremental scanners must return exactly what a fresh full scan returns
//! after files are added, appended to, or removed.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use token_audit_core::claude::{scan_claude, ClaudeScanner};
use token_audit_core::codex::{scan_codex, CodexScanner};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "token-audit-incremental-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to.parent().expect("destination has a parent")).expect("create directory");
    fs::copy(from, to).expect("copy handwritten fixture");
}

fn append(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open fixture for append");
    writeln!(file, "{line}").expect("append line");
}

fn claude_line(request: &str, message: &str, output: u64, second: u32) -> String {
    format!(
        r#"{{"type":"assistant","requestId":"{request}","sessionId":"session-inc","timestamp":"2026-10-02T10:00:{second:02}Z","cwd":"/fake/inc","isSidechain":false,"entrypoint":"cli","message":{{"id":"{message}","model":"claude-test-a","usage":{{"input_tokens":1,"output_tokens":{output},"cache_read_input_tokens":0}}}}}}"#
    )
}

#[test]
fn claude_incremental_matches_full_scan() {
    let home = TempHome::new();
    let projects = home.path().join(".claude/projects");
    let first = projects.join("demo-a/first.jsonl");
    let second = projects.join("demo-b/second.jsonl");
    copy(
        &fixture("claude/home/.claude/projects/demo-a/first.jsonl"),
        &first,
    );
    copy(
        &fixture("claude/home/.claude/projects/demo-b/second.jsonl"),
        &second,
    );

    let mut scanner = ClaudeScanner::default();
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_claude(home.path()).expect("full")
    );
    assert!(scanner.changed());

    // Nothing touched: same result, nothing reparsed.
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_claude(home.path()).expect("full")
    );
    assert!(!scanner.changed());

    // Appending a streamed line that repeats a key from the first file, with more
    // output, must replace the earlier event in place across files.
    append(&second, &claude_line("req-stream", "msg-stream", 99, 30));
    append(&second, &claude_line("req-new", "msg-new", 5, 31));
    append(&second, "{not json");
    let incremental = scanner.scan(home.path()).expect("scan");
    assert!(scanner.changed());
    assert_eq!(incremental, scan_claude(home.path()).expect("full"));

    // A new transcript, including a subagent one.
    let added = projects.join("demo-c/session/subagents/agent.jsonl");
    fs::create_dir_all(added.parent().expect("parent")).expect("create subagent dir");
    fs::write(
        &added,
        format!("{}\n", claude_line("req-agent", "msg-agent", 7, 40)),
    )
    .expect("write");
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_claude(home.path()).expect("full")
    );
    assert!(scanner.changed());

    // Removing a file drops its events and lets the next file's duplicate win.
    fs::remove_file(&first).expect("remove first transcript");
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_claude(home.path()).expect("full")
    );
    assert!(scanner.changed());

    // The projects directory disappearing is an empty scan.
    fs::remove_dir_all(&projects).expect("remove projects");
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_claude(home.path()).expect("full")
    );
    assert!(scanner.changed());
}

#[test]
fn codex_incremental_matches_full_scan() {
    let home = TempHome::new();
    let day = home.path().join(".codex/sessions/2026/10/14");
    let a = day.join("rollout-a.jsonl");
    let z = home
        .path()
        .join(".codex/sessions/2026/10/15/rollout-z.jsonl");
    copy(&fixture("codex/new.jsonl"), &a);
    copy(
        &fixture("codex/fallback.jsonl"),
        &day.join("rollout-b.jsonl"),
    );
    copy(&fixture("codex/duplicate.jsonl"), &z);
    copy(
        &fixture("m7a/session_index.jsonl"),
        &home.path().join(".codex/session_index.jsonl"),
    );

    let mut scanner = CodexScanner::default();
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_codex(home.path()).expect("full")
    );
    assert!(scanner.changed());
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_codex(home.path()).expect("full")
    );
    assert!(!scanner.changed());

    // A later file repeating a response ID from an earlier one is deduplicated.
    let repeat = r#"{"timestamp":"2026-10-15T12:00:00Z","type":"token_usage_record","payload":{"session_id":"fake-z","response_id":"resp-first","usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":1,"total_tokens":6}}}"#;
    append(&z, repeat);
    append(&z, "{broken");
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_codex(home.path()).expect("full")
    );
    assert!(scanner.changed());

    // New rollout in archived sessions.
    copy(
        &fixture("codex/old.jsonl"),
        &home
            .path()
            .join(".codex/archived_sessions/rollout-old.jsonl"),
    );
    assert_eq!(
        scanner.scan(home.path()).expect("scan"),
        scan_codex(home.path()).expect("full")
    );

    // Removing the first file lets the repeated response in the later file count.
    fs::remove_file(&a).expect("remove rollout");
    let incremental = scanner.scan(home.path()).expect("scan");
    assert!(scanner.changed());
    assert_eq!(incremental, scan_codex(home.path()).expect("full"));
    assert!(incremental
        .events
        .iter()
        .any(|event| event.request_id.as_deref() == Some("resp-first")));
}
