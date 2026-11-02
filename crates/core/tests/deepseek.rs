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

#[test]
fn yuan_prices_follow_beijing_peak_hours() {
    use chrono::{TimeZone, Utc};
    use token_audit_core::model::TokenCounts;
    use token_audit_core::pricing::Pricing;

    let pricing = Pricing::bundled().expect("pricing");
    let deepseek = &pricing.deepseek;
    let tokens = TokenCounts {
        input: 9007,
        output: 98,
        ..TokenCounts::default()
    };
    // 2026-11-01 is a Sunday: off-peak all day.
    let sunday = Utc.with_ymd_and_hms(2026, 11, 1, 6, 0, 0).unwrap();
    assert!(!deepseek.is_peak(sunday));
    let cost = deepseek
        .cost_cny("deepseek-flash", &tokens, sunday)
        .expect("priced");
    assert!((cost - (9007.0 * 1.0 + 98.0 * 4.0) / 1e6).abs() < 1e-12);
    // Monday 11:59 Beijing is peak, 12:00 is not; 14:00 is peak again.
    let before_noon = Utc.with_ymd_and_hms(2026, 11, 2, 3, 59, 0).unwrap();
    let noon = Utc.with_ymd_and_hms(2026, 11, 2, 4, 0, 0).unwrap();
    let afternoon = Utc.with_ymd_and_hms(2026, 11, 2, 6, 0, 0).unwrap();
    assert!(deepseek.is_peak(before_noon));
    assert!(!deepseek.is_peak(noon));
    assert!(deepseek.is_peak(afternoon));
    let peak = deepseek
        .cost_cny("deepseek-flash", &tokens, before_noon)
        .expect("priced");
    assert!((peak - 2.0 * cost).abs() < 1e-12);
    // Retired names bill at Flash prices; cache writes bill as misses.
    let alias = TokenCounts {
        cache_read: 1_000_000,
        cache_write_unspecified: 1_000_000,
        ..TokenCounts::default()
    };
    let aliased = deepseek
        .cost_cny("deepseek-v4-flash", &alias, before_noon)
        .expect("alias");
    assert!((aliased - (0.04 + 2.0)).abs() < 1e-9);
    assert!(deepseek.cost_cny("custom-model", &tokens, sunday).is_none());
    assert_eq!(deepseek.title_tokens(999).input, 250);
}

#[test]
fn deepseek_requests_never_have_a_usd_cost() {
    let temp = Temp::new();
    copy(
        "main.zstd",
        &temp.session("workspace", "a", "session.v4.jsonl.zstd"),
    );
    let mut event = scan_deepseek(temp.path()).expect("scan").events.remove(0);
    let pricing = token_audit_core::pricing::Pricing::bundled().expect("pricing");
    // Even a custom route named like a Claude model is not priced in dollars.
    event.model = "claude-opus-5-5".into();
    assert!(pricing.event_cost_usd(&event).is_none());
}

#[test]
fn title_calls_uploads_and_custom_routes_keep_no_content() {
    let temp = Temp::new();
    copy(
        "uploads.zstd",
        &temp.session("one", "a", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    assert_eq!(scan.events.len(), 2);
    assert_eq!(scan.uploads["upload-session"], 2);
    assert_eq!(scan.title_calls.len(), 1);
    let call = &scan.title_calls[0];
    assert_eq!(call.model, "deepseek-flash");
    assert_eq!(call.provider.as_deref(), Some("deepseek-account"));
    assert!(call.bytes > 100);
    assert!(!scan.is_custom_route(&scan.events[0]));
    assert!(scan.is_custom_route(&scan.events[1]));
    let json = serde_json::to_string(&scan).expect("serialize scan");
    assert!(!json.contains("not retained"));
}

#[test]
fn yuan_views_cover_balance_usage_and_tips() {
    use chrono::{TimeZone, Utc};
    use token_audit_core::deepseek_view::{
        deepseek_advice, deepseek_overview, deepseek_usage, DeepSeekCosting, DeepSeekPeriod,
        DeepSeekTipKind, ManualBalance,
    };
    use token_audit_core::i18n::Lang;
    use token_audit_core::pricing::Pricing;

    let temp = Temp::new();
    copy(
        "uploads.zstd",
        &temp.session("one", "a", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    let pricing = Pricing::bundled().expect("pricing");
    let costing = DeepSeekCosting::new(&scan, &pricing.deepseek);
    // Monday 09:00 Beijing: peak prices. The second request went through a custom gateway.
    let official = (100.0 * 2.0 + 10.0 * 8.0) / 1e6;
    assert!((costing.event_cost(&scan.events[0]).expect("priced") - official).abs() < 1e-12);
    assert!(costing.event_cost(&scan.events[1]).is_none());
    let call = &scan.title_calls[0];
    let title = ((call.bytes as f64 / 4.0).ceil() * 2.0 + 32.0 * 8.0) / 1e6;
    assert!((costing.title_cost(call).expect("title") - title).abs() < 1e-12);
    let session = &scan.events[0].session_id;
    let (first, last) = (scan.events[0].timestamp, scan.events[1].timestamp);
    assert!((costing.session_cost(session, first, last).expect("cost") - official).abs() < 1e-12);

    let now = Utc.with_ymd_and_hms(2026, 11, 2, 2, 0, 0).unwrap();
    let manual = ManualBalance {
        amount: 1.0,
        recorded_at: Utc.with_ymd_and_hms(2026, 11, 2, 0, 0, 0).unwrap(),
    };
    let balance = costing.manual_balance(&manual, now);
    assert!((balance.total - (1.0 - official - title)).abs() < 1e-12);
    let overview = deepseek_overview(&costing, Some(balance.clone()), now, &Utc);
    assert_eq!(overview.today_requests, 2);
    assert_eq!(overview.unpriced_requests, 1);
    assert!((overview.daily_average - (official + title)).abs() < 1e-12);
    assert!(overview.days_left.expect("days") > 1000.0);
    assert!(!overview.low_balance);

    let usage = deepseek_usage(
        &costing,
        DeepSeekPeriod::Today,
        Some(balance),
        now,
        &Utc,
        Lang::Zh,
    );
    assert_eq!(usage.requests, 2);
    assert_eq!(usage.title_requests, 1);
    assert_eq!(usage.sessions.len(), 1);
    assert_eq!(usage.projects[0].name, "upload");
    assert!((usage.cost - official - title).abs() < 1e-12);
    assert_eq!(usage.high_requests.len(), 2);

    let advice = deepseek_advice(&costing, 7, &[], now);
    let kinds: Vec<_> = advice.tips.iter().map(|tip| tip.kind).collect();
    assert_eq!(
        kinds,
        [DeepSeekTipKind::LogUpload, DeepSeekTipKind::TitleRequests]
    );
    let dismissed = deepseek_advice(&costing, 7, &["deepseek:log_upload".into()], now);
    assert_eq!(dismissed.tips.len(), 1);
}

#[test]
fn short_session_with_an_uncached_first_request_gets_a_cold_start_tip() {
    use chrono::{TimeZone, Utc};
    use token_audit_core::deepseek_view::{deepseek_advice, DeepSeekCosting, DeepSeekTipKind};

    let temp = Temp::new();
    copy(
        "main.zstd",
        &temp.session("workspace", "a", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    let pricing = token_audit_core::pricing::Pricing::bundled().expect("pricing");
    let costing = DeepSeekCosting::new(&scan, &pricing.deepseek);
    let now = Utc.with_ymd_and_hms(2026, 11, 1, 9, 0, 0).unwrap();
    let advice = deepseek_advice(&costing, 7, &[], now);
    let tip = advice
        .tips
        .iter()
        .find(|tip| tip.kind == DeepSeekTipKind::ColdStart)
        .expect("cold start");
    assert_eq!((tip.sessions, tip.tokens), (1, 9007));
    // Sunday, off-peak: the miss price is ¥1/M and the hit price ¥0.02/M.
    let saving = 9007.0 * (1.0 - 0.02) / 1e6 / 7.0;
    assert!((tip.saving_per_day - saving).abs() < 1e-12);
}

#[test]
fn a_day_without_spend_is_positive_zero() {
    use chrono::{TimeZone, Utc};
    use token_audit_core::deepseek_view::{deepseek_overview, DeepSeekCosting};

    let temp = Temp::new();
    copy(
        "main.zstd",
        &temp.session("workspace", "a", "session.v4.jsonl.zstd"),
    );
    let scan = scan_deepseek(temp.path()).expect("scan");
    let pricing = token_audit_core::pricing::Pricing::bundled().expect("pricing");
    let costing = DeepSeekCosting::new(&scan, &pricing.deepseek);
    let later = Utc.with_ymd_and_hms(2026, 11, 3, 6, 0, 0).unwrap();
    let overview = deepseek_overview(&costing, None, later, &Utc);
    assert_eq!(format!("{:.2}", overview.today_cost), "0.00");
}

/// DeepSeek requests share the event list with Claude and Codex on the desktop,
/// but quota advice, alerts, the share card and the dollar month map ignore them.
#[test]
fn deepseek_stays_out_of_quota_advice_alerts_and_dollar_totals() {
    use chrono::{Duration, TimeZone, Utc};
    use token_audit_core::advice::{build_advice_lang, AdviceConfig, AdviceInput, AlertTracker};
    use token_audit_core::attribution::attribute;
    use token_audit_core::dashboard::{build_overview, OverviewInput};
    use token_audit_core::i18n::Lang;
    use token_audit_core::model::{TokenCounts, UsageEvent};
    use token_audit_core::pricing::Pricing;
    use token_audit_core::recorder::RecorderStatus;
    use token_audit_core::share::share_summary;

    let now = Utc.with_ymd_and_hms(2026, 11, 2, 6, 0, 0).unwrap();
    let event = |tool, source, session: &str, model: &str, minutes: i64, input: u64| UsageEvent {
        tool,
        source,
        session_id: session.into(),
        request_id: None,
        timestamp: now - Duration::minutes(minutes),
        model: model.into(),
        project: Some("/sample/project".into()),
        is_subagent: false,
        fast: false,
        effort: None,
        web_search_requests: 0,
        tokens: TokenCounts {
            input,
            output: 2_000,
            ..TokenCounts::default()
        },
    };
    let mut events = vec![event(
        Tool::Claude,
        Source::ClaudeCli,
        "claude",
        "claude-opus-5-5",
        30,
        5_000,
    )];
    // A long, uncached Harness session: the kind of usage Claude's rules would flag.
    for minute in (1..=20).rev() {
        events.push(event(
            Tool::DeepSeek,
            Source::DeepSeek,
            "session-heavy",
            "claude-opus-5-5",
            minute,
            900_000,
        ));
    }
    let pricing = Pricing::bundled().expect("pricing");
    let titles = std::collections::BTreeMap::new();
    let attribution = attribute(&events, &[], &titles, &[], &[], &pricing);
    let input = OverviewInput {
        events: &events,
        titles: &titles,
        points: &[],
        attribution: &attribution,
        claude_snapshots: &[],
        codex_snapshots: &[],
        codex_log: &[],
        pricing: &pricing,
        recorder: RecorderStatus::default(),
    };
    let overview = build_overview(&input, now, Utc);
    assert!(!overview.month_cost_by_tool.contains_key(&Tool::DeepSeek));
    assert!(overview
        .tools
        .iter()
        .all(|tool| tool.tool != Tool::DeepSeek));
    let share = share_summary(&input, now, Utc);
    assert!(share.tools.iter().all(|tool| tool.tool != Tool::DeepSeek));
    assert!((share.month_estimated_cost_usd - overview.month_estimated_cost_usd).abs() < 1e-9);

    let config = AdviceConfig::bundled().expect("advice config");
    let advice_input = AdviceInput {
        events: &attribution.events,
        prompts: &[],
        pricing: &pricing,
        config: &config,
        dismissed: &[],
        compactions: &[],
        environment: None,
        quotas: &[],
    };
    let report = build_advice_lang(&advice_input, 7, now, Lang::Zh);
    assert!(report.tools.iter().all(|tool| tool.tool != Tool::DeepSeek));
    assert!(report
        .event_tags
        .iter()
        .all(|tag| tag.tool != Tool::DeepSeek));
    let mut tracker = AlertTracker::default();
    tracker.seed(&advice_input, now - Duration::hours(1));
    for minute in 0..60 {
        if let Some(alert) =
            tracker.check_lang(&advice_input, now + Duration::minutes(minute), Lang::Zh)
        {
            assert_ne!(alert.tool, Tool::DeepSeek);
        }
    }
}
