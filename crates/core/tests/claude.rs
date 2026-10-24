use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use token_audit_core::claude::scan_claude;
use token_audit_core::model::{Source, Tool};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "token-audit-claude-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn copy_fixture(&self) {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude/home/.claude/projects");
        for (project, file) in [("demo-a", "first.jsonl"), ("demo-b", "second.jsonl")] {
            let destination = self.0.join(".claude/projects").join(project);
            fs::create_dir_all(&destination).expect("create fixture project");
            fs::copy(fixtures.join(project).join(file), destination.join(file))
                .expect("copy handwritten fixture");
        }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated test home");
    }
}

#[test]
fn scans_and_deduplicates_across_files() {
    let home = TempHome::new();
    home.copy_fixture();
    let scan = scan_claude(home.path()).expect("scan fixture home");
    assert_eq!(scan.stats.files, 2);
    assert_eq!(scan.stats.total_lines, 19);
    assert_eq!(scan.stats.valid_usage_lines, 11);
    assert_eq!(scan.stats.deduplicated_lines, 5);
    assert_eq!(scan.stats.malformed_lines, 4);
    assert_eq!(scan.stats.non_usage_lines, 4);
    assert_eq!(scan.stats.unreadable_files, 0);
    assert_eq!(scan.events.len(), 6);
    assert!(scan
        .events
        .windows(2)
        .all(|pair| pair[0].timestamp <= pair[1].timestamp));

    let stream = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("req-stream"))
        .expect("stream event");
    assert_eq!(stream.tool, Tool::Claude);
    assert_eq!(stream.source, Source::ClaudeCli);
    assert_eq!(stream.tokens.output, 12);
    assert_eq!(stream.tokens.cache_read, 7);
    assert_eq!(stream.tokens.cache_write_5m, 4);
    assert_eq!(stream.tokens.cache_write_1h, 6);
    assert_eq!(stream.tokens.cache_write_unspecified, 0);
    assert_eq!(stream.tokens.reasoning, 0);
    assert_eq!(stream.project.as_deref(), Some("/fake/project-a"));

    let desktop = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("req-resume"))
        .expect("resume event");
    assert_eq!(desktop.source, Source::ClaudeDesktop);
    assert!(desktop.is_subagent);
    assert_eq!(desktop.tokens.output, 5);

    let unknown = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("req-unknown"))
        .expect("unknown source event");
    assert_eq!(unknown.source, Source::Unknown);
    assert_eq!(unknown.tokens.cache_write_unspecified, 8);
    assert_eq!(unknown.project, None);

    let request_only = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("req-alone"))
        .expect("request-only event");
    assert_eq!(request_only.tokens.output, 8);
    assert_eq!(
        scan.events
            .iter()
            .filter(|event| event.request_id.is_none())
            .count(),
        2
    );

    let summary = scan.debug_summary();
    assert_eq!(summary.unique_sessions, 3);
    assert_eq!(summary.events_total, 6);
    assert_eq!(summary.tokens.input, 9);
    assert_eq!(summary.tokens.output, 33);
    assert_eq!(summary.tokens.cache_read, 7);
    assert_eq!(summary.tokens.cache_write_5m, 7);
    assert_eq!(summary.tokens.cache_write_1h, 6);
    assert_eq!(summary.tokens.cache_write_unspecified, 8);
    assert_eq!(summary.sources.claude_cli, 2);
    assert_eq!(summary.sources.claude_desktop, 2);
    assert_eq!(summary.sources.unknown, 2);
    assert_eq!(summary.models["claude-test-a"].events, 2);
    assert_eq!(summary.models["claude-test-a"].output, 20);
    assert_eq!(summary.models["claude-test-b"].events, 4);
    assert_eq!(summary.models["claude-test-b"].output, 13);
}

#[test]
fn missing_and_empty_project_directories_are_empty_scans() {
    let home = TempHome::new();
    let missing = scan_claude(home.path()).expect("missing projects directory");
    assert!(missing.events.is_empty());
    assert_eq!(missing.stats.total_lines, 0);

    fs::create_dir_all(home.path().join(".claude/projects")).expect("create empty projects");
    let empty = scan_claude(home.path()).expect("empty projects directory");
    assert!(empty.events.is_empty());
    assert_eq!(empty.stats.files, 0);
}

#[cfg(unix)]
#[test]
fn unreadable_file_does_not_abort_other_files() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempHome::new();
    home.copy_fixture();
    let broken = home.path().join(".claude/projects/demo-a/broken.jsonl");
    fs::write(&broken, "{}\n").expect("create unreadable fixture file");
    fs::set_permissions(&broken, fs::Permissions::from_mode(0o000))
        .expect("make fixture unreadable");

    let scan = scan_claude(home.path()).expect("scan despite unreadable file");
    fs::set_permissions(&broken, fs::Permissions::from_mode(0o600))
        .expect("restore permissions for cleanup");
    assert_eq!(scan.stats.files, 3);
    assert_eq!(scan.stats.unreadable_files, 1);
    assert_eq!(scan.events.len(), 6);
}

#[test]
fn equal_output_duplicates_keep_first_sorted_path_and_line() {
    let home = TempHome::new();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude/tie/home/.claude/projects");
    for (project, file) in [("z-project", "a.jsonl"), ("a-project", "z.jsonl")] {
        let destination = home.path().join(".claude/projects").join(project);
        fs::create_dir_all(&destination).expect("create fixture project");
        fs::copy(fixtures.join(project).join(file), destination.join(file))
            .expect("copy handwritten fixture");
    }

    let first = scan_claude(home.path()).expect("first scan");
    assert_eq!(first.stats.files, 2);
    assert_eq!(first.stats.valid_usage_lines, 3);
    assert_eq!(first.stats.deduplicated_lines, 2);
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].session_id, "session-first");
    assert_eq!(first.events[0].source, Source::ClaudeCli);
    assert_eq!(first.debug_summary().unique_sessions, 1);

    for _ in 0..5 {
        let repeated = scan_claude(home.path()).expect("repeated scan");
        assert_eq!(repeated.stats, first.stats);
        assert_eq!(repeated.events, first.events);
    }
}

#[test]
fn nested_subagent_transcripts_are_scanned() {
    let home = TempHome::new();
    let session_dir = home
        .path()
        .join(".claude/projects/demo-sub/fake-session/subagents");
    fs::create_dir_all(&session_dir).expect("create nested subagent folder");
    fs::write(
        home.path().join(".claude/projects/demo-sub/fake-session.jsonl"),
        r#"{"type":"assistant","sessionId":"fake-session","requestId":"req-main","timestamp":"2026-10-01T10:00:00Z","entrypoint":"cli","message":{"id":"msg-main","model":"claude-test","usage":{"input_tokens":1,"output_tokens":2}}}
"#,
    )
    .expect("write main transcript");
    fs::write(
        session_dir.join("agent-fake.jsonl"),
        r#"{"type":"assistant","sessionId":"fake-session","requestId":"req-sub","timestamp":"2026-10-01T10:00:05Z","entrypoint":"cli","isSidechain":true,"message":{"id":"msg-sub","model":"claude-test","usage":{"input_tokens":3,"output_tokens":4}}}
"#,
    )
    .expect("write subagent transcript");
    let scan = scan_claude(home.path()).expect("scan nested home");
    assert_eq!(scan.stats.files, 2);
    assert_eq!(scan.events.len(), 2);
    let subagent = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("req-sub"))
        .expect("subagent event is scanned");
    assert!(subagent.is_subagent);
    assert_eq!(subagent.session_id, "fake-session");
}
