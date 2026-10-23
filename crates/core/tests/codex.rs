use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use token_audit_core::codex::scan_codex;
use token_audit_core::model::{Source, Tool};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "token-audit-codex-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated test home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn copy_fixture(&self, fixture: &str, day: &str, filename: &str) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex")
            .join(fixture);
        let destination = self.0.join(".codex/sessions/2026/10").join(day);
        fs::create_dir_all(&destination).expect("create fixture day");
        fs::copy(source, destination.join(filename)).expect("copy handwritten fixture");
    }

    fn copy_all(&self) {
        self.copy_fixture("duplicate.jsonl", "15", "rollout-z.jsonl");
        self.copy_fixture("new.jsonl", "14", "rollout-a.jsonl");
        self.copy_fixture("old.jsonl", "14", "rollout-b.jsonl");
        self.copy_fixture("sources.jsonl", "16", "rollout-a.jsonl");
        self.copy_fixture("cli-rs.jsonl", "17", "rollout-a.jsonl");
        self.copy_fixture("unknown.jsonl", "18", "rollout-a.jsonl");
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated test home");
    }
}

#[test]
fn scans_records_fallback_sources_and_rate_limits() {
    let home = TempHome::new();
    home.copy_all();
    let scan = scan_codex(home.path()).expect("scan fixtures");
    assert_eq!(scan.stats.files, 6);
    assert_eq!(scan.stats.total_lines, 27);
    assert_eq!(scan.stats.malformed_lines, 2);
    assert_eq!(scan.stats.non_usage_lines, 15);
    assert_eq!(scan.stats.usage_records, 8);
    assert_eq!(scan.stats.token_count_usage_lines, 3);
    assert_eq!(scan.stats.deduplicated_lines, 2);
    assert_eq!(scan.stats.files_with_records, 5);
    assert_eq!(scan.stats.files_token_count_only, 1);
    assert_eq!(scan.stats.rate_limit_snapshots, 3);
    assert_eq!(scan.stats.unreadable_files, 0);
    assert_eq!(scan.events.len(), 8);
    assert_eq!(scan.rate_limits.len(), 3);
    assert!(scan
        .events
        .windows(2)
        .all(|pair| pair[0].timestamp <= pair[1].timestamp));

    let first = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("resp-first"))
        .expect("first response");
    assert_eq!(first.tool, Tool::Codex);
    assert_eq!(first.source, Source::CodexDesktop);
    assert_eq!(first.session_id, "fake-record-session");
    assert_eq!(first.project.as_deref(), Some("/fake/turn-a"));
    assert!(first.is_subagent);
    assert_eq!(first.model, "gpt-fake-a");
    assert_eq!(first.tokens.input, 20);
    assert_eq!(first.tokens.cache_read, 80);
    assert_eq!(first.tokens.cache_write_unspecified, 3);
    assert_eq!(first.tokens.cache_write_5m, 0);
    assert_eq!(first.tokens.cache_write_1h, 0);
    assert_eq!(first.tokens.output, 12);
    assert_eq!(first.tokens.reasoning, 4);

    let second = scan
        .events
        .iter()
        .find(|event| event.request_id.as_deref() == Some("resp-second"))
        .expect("second response");
    assert_eq!(second.model, "gpt-fake-b");
    assert_eq!(second.tokens.input, 0);
    assert_eq!(second.tokens.cache_read, 10);
    assert_eq!(second.project.as_deref(), Some("/fake/turn-a"));
    assert_eq!(second.session_id, "fake-new");

    let old = scan
        .events
        .iter()
        .find(|event| event.session_id == "fake-old")
        .expect("old response");
    assert_eq!(old.source, Source::CodexCli);
    assert!(old.is_subagent);
    assert_eq!(old.request_id, None);
    assert_eq!(old.model, "gpt-old");
    assert_eq!(
        scan.events
            .iter()
            .filter(|event| event.session_id == "fake-old")
            .count(),
        2
    );
    assert_eq!(
        scan.events
            .iter()
            .filter(|event| event.request_id.as_deref() == Some("resp-first"))
            .count(),
        1
    );
    assert_eq!(
        scan.events
            .iter()
            .find(|event| event.request_id.as_deref() == Some("resp-third"))
            .map(|event| event.source),
        Some(Source::CodexCli)
    );
    let unknown = scan
        .events
        .iter()
        .find(|event| event.session_id == "fake-unknown")
        .expect("unknown source");
    assert_eq!(unknown.source, Source::Unknown);
    assert_eq!(unknown.model, "");

    assert_eq!(scan.rate_limits[0].limit_id.as_deref(), Some("codex"));
    assert_eq!(scan.rate_limits[0].plan_type.as_deref(), Some("plus"));
    let primary = scan.rate_limits[0]
        .primary
        .as_ref()
        .expect("primary window");
    let secondary = scan.rate_limits[0]
        .secondary
        .as_ref()
        .expect("secondary window");
    assert_eq!(primary.used_percent, 12.5);
    assert_eq!(primary.window_minutes, Some(300));
    assert_eq!(primary.resets_at, Utc.timestamp_opt(1791977148, 0).single());
    assert_eq!(secondary.window_minutes, Some(10080));
    assert!(scan.rate_limits[1].secondary.is_none());
    assert!(scan.rate_limits[2].primary.is_none());
    assert_eq!(
        scan.rate_limits[2]
            .secondary
            .as_ref()
            .map(|window| window.used_percent),
        Some(4.0)
    );

    let summary = scan.debug_summary();
    assert_eq!(summary.unique_sessions, 7);
    assert_eq!(summary.tokens.input, 33);
    assert_eq!(summary.tokens.output, 23);
    assert_eq!(summary.tokens.reasoning, 5);
    assert_eq!(summary.tokens.cache_read, 90);
    assert_eq!(summary.tokens.cache_write_unspecified, 3);
    assert_eq!(summary.sources.codex_cli, 4);
    assert_eq!(summary.sources.codex_desktop, 3);
    assert_eq!(summary.sources.unknown, 1);
    assert_eq!(summary.models["gpt-fake-a"].output, 12);
    assert_eq!(
        summary
            .latest_rate_limit
            .as_ref()
            .and_then(|rate| rate.secondary.as_ref())
            .map(|window| window.used_percent),
        Some(4.0)
    );
}

#[test]
fn missing_directory_and_repeat_scans_are_deterministic() {
    let home = TempHome::new();
    let missing = scan_codex(home.path()).expect("missing directory");
    assert_eq!(missing.stats.files, 0);
    assert!(missing.events.is_empty());
    assert!(missing.rate_limits.is_empty());
    home.copy_all();
    let first = scan_codex(home.path()).expect("first scan");
    for _ in 0..3 {
        let next = scan_codex(home.path()).expect("repeat scan");
        assert_eq!(next, first);
    }
}

#[test]
fn unreadable_file_is_counted_and_filename_uuid_is_fallback() {
    let home = TempHome::new();
    home.copy_fixture("unknown.jsonl", "14", "rollout-a.jsonl");
    home.copy_fixture("duplicate.jsonl", "15", "rollout-a.jsonl");
    let sessions = home.path().join(".codex/sessions/2026/10/16");
    fs::create_dir_all(&sessions).expect("create day");
    fs::create_dir(sessions.join("rollout-broken.jsonl")).expect("create unreadable entry");
    let scan = scan_codex(home.path()).expect("scan with unreadable entry");
    assert_eq!(scan.stats.unreadable_files, 1);
    assert_eq!(scan.events.len(), 3);

    home.copy_fixture(
        "fallback.jsonl",
        "17",
        "rollout-2026-10-17T10-00-00-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl",
    );
    let scan = scan_codex(home.path()).expect("scan filename fallback");
    assert_eq!(
        scan.events.last().map(|event| event.session_id.as_str()),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
}

#[test]
fn equal_timestamps_keep_sorted_path_order_for_events_and_snapshots() {
    let home = TempHome::new();
    home.copy_fixture("tie-b.jsonl", "14", "rollout-b.jsonl");
    home.copy_fixture("tie-a.jsonl", "14", "rollout-a.jsonl");
    let scan = scan_codex(home.path()).expect("scan equal timestamps");
    assert_eq!(scan.events.len(), 2);
    assert_eq!(scan.events[0].request_id.as_deref(), Some("resp-tie-a"));
    assert_eq!(scan.events[1].request_id.as_deref(), Some("resp-tie-b"));
    assert_eq!(scan.rate_limits.len(), 2);
    assert_eq!(scan.rate_limits[0].session_id, "fake-tie-a");
    assert_eq!(scan.rate_limits[1].session_id, "fake-tie-b");
}

#[test]
fn null_info_does_not_reset_old_format_total_deduplication() {
    let home = TempHome::new();
    home.copy_fixture(
        "null-between-totals.jsonl",
        "14",
        "rollout-null-between-totals.jsonl",
    );
    let scan = scan_codex(home.path()).expect("scan old format with null info");
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.events[0].tokens.input, 10);
    assert_eq!(scan.stats.token_count_usage_lines, 2);
    assert_eq!(scan.stats.deduplicated_lines, 1);
}
