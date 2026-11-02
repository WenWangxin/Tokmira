use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use token_audit_core::deepseek::{deepseek_root, scan_deepseek, DeepSeekScanner};
use token_audit_core::detect::{detect_deepseek_install, DeepSeekInstall, DeepSeekPlatform};
use token_audit_core::model::{Source, Tool};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-deepseek-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary directory");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn session(&self, workspace: &str, session: &str, name: &str) -> PathBuf {
        let path = self
            .0
            .join("sessions")
            .join(workspace)
            .join(format!("session-{session}"))
            .join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("session directory");
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/deepseek")
        .join(name)
}
fn copy(name: &str, to: &Path) {
    fs::copy(fixture(name), to).expect("copy handwritten fixture");
}

#[test]
fn v4_multiframe_replaces_attempt_and_counts_retry() {
    let temp = Temp::new();
    copy(
        "main.zstd",
        &temp.session("workspace", "a", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    assert_eq!(scan.events.len(), 2);
    assert_eq!(scan.stats.files, 1);
    assert_eq!(scan.stats.usage_samples, 3);
    assert_eq!(scan.stats.replaced_samples, 1);
    assert_eq!(scan.stats.retries, 1);
    assert_eq!(scan.stats.title_requests, 1);
    assert_eq!(scan.stats.malformed_lines, 1);
    assert_eq!(scan.titles["synthetic-session"], "Final synthetic title");
    assert!(scan.session_providers["synthetic-session"].contains("deepseek-account"));
    let first = &scan.events[0];
    assert_eq!(first.tool, Tool::DeepSeek);
    assert_eq!(first.source, Source::DeepSeek);
    assert_eq!(first.session_id, "synthetic-session");
    assert_eq!(first.request_id.as_deref(), Some("request-1"));
    assert_eq!(first.timestamp.timestamp_millis(), 1793520002000);
    assert_eq!(first.model, "deepseek-flash");
    assert_eq!(first.effort.as_deref(), Some("medium"));
    assert_eq!(first.project.as_deref(), Some("/sample/project"));
    assert_eq!(
        (
            first.tokens.input,
            first.tokens.output,
            first.tokens.cache_read,
            first.tokens.cache_write_unspecified
        ),
        (9007, 98, 0, 0)
    );
    assert_eq!(
        (
            scan.events[1].tokens.input,
            scan.events[1].tokens.output,
            scan.events[1].tokens.cache_read,
            scan.events[1].tokens.cache_write_unspecified
        ),
        (2, 3, 1, 4)
    );
    let summary = scan.debug_summary();
    assert_eq!(summary.tokens.input, 9009);
    let json = serde_json::to_string(&summary).expect("serialize summary");
    assert!(!json.contains("not retained"));
}

#[test]
fn seeded_custom_route_and_unsupported_version() {
    let temp = Temp::new();
    copy(
        "seeded.zstd",
        &temp.session("one", "a", "session.v4.jsonl.zstd"),
    );
    copy(
        "v3.zstd",
        &temp.session("one", "b", "session.v3.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.events[0].tokens.input, 7);
    assert!(scan.events[0].is_subagent);
    assert_eq!(scan.events[0].model, "custom-model");
    assert_eq!(scan.stats.seeded_events_skipped, 2);
    assert_eq!(scan.stats.unsupported_files, 1);
    assert_eq!(scan.stats.custom_route_events, 1);
}

#[test]
fn final_stream_usage_uses_last_chunk_and_identical_sample_is_ignored() {
    let temp = Temp::new();
    copy(
        "fallback.zstd",
        &temp.session("one", "a", "session.v4.jsonl.zstd"),
    );
    copy(
        "version5.zstd",
        &temp.session("one", "b", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    assert_eq!(scan.stats.unsupported_files, 1);
    // Both title requests in the session are billed, so both count.
    assert_eq!(scan.stats.title_requests, 2);
    assert_eq!(scan.stats.malformed_lines, 0);
    assert_eq!(scan.stats.non_usage_lines, 4);
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.stats.usage_samples, 2);
    assert_eq!(scan.stats.replaced_samples, 0);
    assert_eq!(scan.events[0].model, "fallback-model");
    assert_eq!(scan.events[0].timestamp.timestamp_millis(), 1793520101000);
    assert_eq!(
        (
            scan.events[0].tokens.input,
            scan.events[0].tokens.output,
            scan.events[0].tokens.cache_read,
            scan.events[0].tokens.cache_write_unspecified
        ),
        (3, 4, 5, 6)
    );
    assert_eq!(scan.stats.custom_route_events, 0);
}

#[test]
fn truncated_tail_preserves_complete_frames() {
    let temp = Temp::new();
    let file = temp.session("one", "a", "session.v4.jsonl.zstd");
    let compressed = fs::read(fixture("main.zstd")).expect("fixture");
    let first_frame = fs::read(fixture("main-frame1.zstd")).expect("first frame");
    let second = &compressed[first_frame.len()..];
    let mut bytes = first_frame;
    bytes.extend_from_slice(&second[..second.len() / 2]);
    fs::write(file, bytes).expect("write truncated fixture");
    let scan = scan_deepseek(temp.path()).expect("scan");
    assert_eq!(scan.stats.truncated_files, 1);
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.events[0].tokens.input, 9007);
}

#[test]
fn unchanged_files_reuse_cached_scan() {
    let temp = Temp::new();
    let first = temp.session("one", "a", "session.v4.jsonl.zstd");
    copy("main.zstd", &first);
    let mut scanner = DeepSeekScanner::default();
    let initial = scanner.scan(temp.path()).expect("initial");
    assert!(scanner.changed());
    assert_eq!(scanner.scan(temp.path()).expect("unchanged"), initial);
    assert!(!scanner.changed());
    let second = temp.session("one", "b", "session.v4.jsonl.zstd");
    copy("seeded.zstd", &second);
    assert_eq!(scanner.scan(temp.path()).expect("added").events.len(), 3);
    assert!(scanner.changed());
    fs::remove_file(second).expect("remove");
    assert_eq!(scanner.scan(temp.path()).expect("removed"), initial);
    assert!(scanner.changed());
}

#[test]
fn root_and_install_detection_use_only_supplied_paths() {
    let temp = Temp::new();
    let home = temp.path().join("home");
    let root = temp.path().join("custom");
    let app = temp.path().join("Applications/DeepSeek Harness.app");
    let cli = temp.path().join("usr/local/bin/dsh");
    let path_dir = temp.path().join("bin");
    assert_eq!(deepseek_root(&home, None), home.join(".dsh"));
    assert_eq!(
        deepseek_root(&home, Some(OsStr::new("  "))),
        home.join(".dsh")
    );
    assert_eq!(deepseek_root(&home, Some(root.as_os_str())), root);
    let detect = |platform, path: Option<&OsStr>| {
        detect_deepseek_install(&root, &home, &app, &cli, path, platform)
    };
    assert_eq!(
        detect(DeepSeekPlatform::Mac, None),
        DeepSeekInstall::default()
    );
    fs::create_dir_all(&app).expect("app");
    assert!(detect(DeepSeekPlatform::Mac, None).desktop);
    assert!(!detect(DeepSeekPlatform::Windows, None).desktop);
    fs::create_dir_all(cli.parent().expect("parent")).expect("bin");
    fs::write(&cli, b"").expect("cli");
    assert!(detect(DeepSeekPlatform::Mac, None).cli);
    assert!(!detect(DeepSeekPlatform::Windows, None).cli);
    fs::create_dir_all(&path_dir).expect("PATH");
    fs::write(path_dir.join("dsh.cmd"), b"").expect("cmd");
    assert!(detect(DeepSeekPlatform::Windows, Some(path_dir.as_os_str())).cli);
    fs::create_dir_all(root.join("profiles/desktop")).expect("desktop profile");
    fs::create_dir_all(root.join("profiles/headless")).expect("headless profile");
    assert_eq!(
        detect(DeepSeekPlatform::Windows, None),
        DeepSeekInstall {
            desktop: true,
            cli: true
        }
    );
}
