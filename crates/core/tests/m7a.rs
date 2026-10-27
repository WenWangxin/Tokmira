use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, TimeZone, Utc};
use token_audit_core::attribution::{attribute, WindowKind};
use token_audit_core::claude::scan_claude;
use token_audit_core::codex::scan_codex;
use token_audit_core::dashboard::{build_overview, OverviewInput};
use token_audit_core::forecast::{forecast, Forecast, WindowKind as ForecastKind};
use token_audit_core::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{
    append_claude, read_claude, ClaudeSnapshot, CodexSnapshot, StoredWindow,
};
use token_audit_core::recorder::{QuotaWindow, RecorderStatus, StatuslineRecord};
use token_audit_core::settings::{
    read_settings, toggle_menu_bar_tool, write_settings, AppSettings, Calibration, Theme,
};
use token_audit_core::timeline::{build_timeline, window_id, Origin, QuotaPoint, Window};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-m7a-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary directory");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn fixture(&self, name: &str, relative: &str) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m7a")
            .join(name);
        let target = self.path().join(relative);
        fs::create_dir_all(target.parent().expect("fixture parent")).expect("fixture directory");
        fs::copy(source, target).expect("copy handwritten fixture");
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temporary directory");
    }
}
fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 24, hour, minute, 0)
        .single()
        .expect("fixed time")
}
fn event(minute: u32, input: u64, model: &str) -> UsageEvent {
    UsageEvent {
        tool: Tool::Claude,
        source: Source::ClaudeCli,
        session_id: "session-a".into(),
        request_id: Some(format!("request-{minute}")),
        timestamp: at(1, minute),
        model: model.into(),
        project: Some("fixture".into()),
        is_subagent: false,
        fast: false,
        effort: None,
        web_search_requests: 0,
        tokens: TokenCounts {
            input,
            ..TokenCounts::default()
        },
    }
}
fn point(minute: u32, percent: f64, reset: DateTime<Utc>) -> QuotaPoint {
    QuotaPoint {
        tool: Tool::Claude,
        at: at(1, minute),
        origin: Origin::Official,
        five_hour: Some(Window {
            used_percent: percent,
            resets_at: Some(reset),
            window_minutes: Some(300),
        }),
        weekly: None,
    }
}
fn approx(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn recognizes_titles_and_prompts_without_changing_usage() {
    let temp = Temp::new();
    temp.fixture("claude.jsonl", ".claude/projects/fixture/session-a.jsonl");
    temp.fixture("codex.jsonl", ".codex/sessions/2026/10/24/rollout-a.jsonl");
    temp.fixture("session_index.jsonl", ".codex/session_index.jsonl");
    let claude = scan_claude(temp.path()).expect("Claude fixture scan");
    assert_eq!(claude.events.len(), 1);
    assert_eq!(claude.events[0].tokens.output, 20);
    assert_eq!(claude.prompts.len(), 2);
    assert_eq!(claude.prompts[0].timestamp, at(10, 0));
    assert_eq!(
        claude.titles.get("session-a").map(String::as_str),
        Some("最终自定义")
    );
    let codex = scan_codex(temp.path()).expect("Codex fixture scan");
    assert_eq!(codex.events.len(), 1);
    assert_eq!(codex.prompts.len(), 1);
    assert_eq!(codex.prompts[0].timestamp, at(10, 1));
    assert_eq!(
        codex.titles.get("session-b").map(String::as_str),
        Some("新标题")
    );
}

#[test]
fn codex_task_started_is_one_prompt_with_same_time_legacy_event() {
    let temp = Temp::new();
    temp.fixture("codex.jsonl", ".codex/sessions/2026/10/24/rollout-a.jsonl");
    let codex = scan_codex(temp.path()).expect("Codex fixture scan");
    assert_eq!(codex.prompts.len(), 1);
    assert_eq!(codex.prompts[0].timestamp, at(10, 1));

    let legacy = Temp::new();
    legacy.fixture(
        "codex-old-prompt.jsonl",
        ".codex/sessions/2026/10/24/rollout-b.jsonl",
    );
    let codex = scan_codex(legacy.path()).expect("legacy Codex fixture scan");
    assert_eq!(codex.prompts.len(), 1);
    assert_eq!(codex.prompts[0].timestamp, at(10, 2));
}

#[test]
fn timeline_deduplicates_only_when_both_windows_match_and_rounds_reset() {
    let reset = at(5, 0) + Duration::seconds(40);
    let snapshot = |minute, five, weekly| ClaudeSnapshot {
        at: at(1, minute),
        subscription_type: None,
        rate_limit_tier: None,
        five_hour: Some(StoredWindow {
            used_percent: five,
            resets_at: Some(reset),
            window_minutes: Some(300),
        }),
        seven_day: Some(StoredWindow {
            used_percent: weekly,
            resets_at: Some(at(7, 0)),
            window_minutes: Some(10080),
        }),
        weekly_breakdown: None,
    };
    let points = build_timeline(
        &[
            snapshot(1, 1.0, 2.0),
            snapshot(2, 1.0, 2.0),
            snapshot(3, 1.0, 3.0),
        ],
        &[],
        &[],
        &[],
        None,
    );
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].at, at(1, 1));
    assert_eq!(
        window_id(points[0].five_hour.as_ref().expect("window")),
        Some(at(5, 0))
    );
    let statusline = StatuslineRecord {
        recorded_at: at(1, 2),
        session_id: None,
        version: None,
        model: None,
        five_hour: Some(QuotaWindow {
            used_percentage: Some(2.0),
            resets_at: Some(reset.timestamp()),
        }),
        seven_day: None,
        prompt_cache: None,
    };
    let merged = build_timeline(&[snapshot(1, 1.0, 2.0)], &[], &[statusline], &[], None);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].origin, Origin::Official);
    assert_eq!(merged[1].origin, Origin::Statusline);
    let calibrated = build_timeline(
        &[],
        &[],
        &[],
        &[],
        Some(&Calibration {
            tool: Tool::Claude,
            five_hour_percent: 7.0,
            recorded_at: at(1, 4),
        }),
    );
    assert_eq!(calibrated[0].origin, Origin::Manual);
    assert_eq!(
        calibrated[0].five_hour.as_ref().map(|w| w.used_percent),
        Some(7.0)
    );
}

#[test]
fn timeline_ignores_other_codex_limits_without_changing_snapshot_count() {
    let temp = Temp::new();
    temp.fixture(
        "codex-limits.jsonl",
        ".codex/sessions/2026/10/24/rollout-limits.jsonl",
    );
    let scan = scan_codex(temp.path()).expect("Codex limits fixture scan");
    assert_eq!(scan.stats.rate_limit_snapshots, 3);
    assert_eq!(scan.rate_limits.len(), 3);
    let app = |limit_id: Option<&str>, minute, percent| CodexSnapshot {
        at: at(10, minute),
        limit_id: limit_id.map(str::to_owned),
        plan_type: None,
        primary: Some(StoredWindow {
            used_percent: percent,
            resets_at: Some(at(14, 0)),
            window_minutes: Some(300),
        }),
        secondary: None,
        reset_credits_available: None,
    };
    let points = build_timeline(
        &[],
        &[
            app(Some("premium"), 0, 90.0),
            app(Some("codex"), 1, 1.0),
            app(None, 2, 2.0),
        ],
        &[],
        &scan.rate_limits,
        None,
    );
    assert_eq!(points.len(), 4);
    assert!(points
        .iter()
        .all(|p| p.five_hour.as_ref().is_some_and(|w| w.used_percent < 50.0)));
    let old: CodexSnapshot = serde_json::from_str(
        r#"{"at":"2026-10-24T10:00:00Z","plan_type":null,"primary":null,"secondary":null,"reset_credits_available":null}"#,
    ).expect("old stored snapshot");
    assert_eq!(old.limit_id, None);
}

#[test]
fn unchanged_percentages_keep_all_requests_in_the_interval() {
    let events = vec![
        event(1, 100, "claude-opus-5"),
        event(2, 200, "claude-opus-5"),
        event(3, 300, "claude-opus-5"),
    ];
    let points = vec![
        point(1, 0.0, at(5, 0)),
        point(2, 0.0, at(5, 0)),
        point(3, 0.0, at(5, 0)),
        point(4, 1.0, at(5, 0)),
    ];
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "session-a".into(),
        timestamp: at(1, 0),
    }];
    let titles = BTreeMap::from([("session-a".into(), "手写标题".into())]);
    let result = attribute(
        &events,
        &prompts,
        &titles,
        &points,
        &[],
        &Pricing::bundled().expect("pricing"),
    );
    approx(result.events[0].five_hour_delta, 1.0 / 6.0);
    approx(result.events[1].five_hour_delta, 1.0 / 3.0);
    approx(result.events[2].five_hour_delta, 0.5);
    approx(result.windows[0].attributed_total, 1.0);
    assert_eq!(result.windows[0].points, 4);
    assert_eq!(result.prompts[0].requests, 3);
    assert_eq!(result.prompts[0].tokens.input, 600);
    assert_eq!(result.prompts[0].title.as_deref(), Some("手写标题"));
    assert_eq!(result.window_prompts[0].requests, 3);
}

#[test]
fn priced_requests_use_cost_weights_and_unpriced_requests_get_zero_weight() {
    let events = vec![
        event(1, 100, "claude-opus-5"),
        event(2, 100, "claude-haiku-5-5"),
        event(3, 100, "unknown-model"),
    ];
    let result = attribute(
        &events,
        &[],
        &BTreeMap::new(),
        &[point(4, 51.0, at(5, 0))],
        &[],
        &Pricing::bundled().expect("pricing"),
    );
    approx(result.events[0].five_hour_delta, 50.0);
    approx(result.events[1].five_hour_delta, 1.0);
    approx(result.events[2].five_hour_delta, 0.0);
    assert_eq!(result.prompts[0].requests, 3);
    assert!(result.prompts[0].prompt_at.is_none());
}

#[test]
fn actual_reset_seconds_exclude_request_before_new_window() {
    let reset = at(6, 0) + Duration::seconds(40);
    let mut before = event(0, 100, "unknown-model");
    before.timestamp = at(1, 0) + Duration::seconds(15);
    let mut after = event(0, 100, "unknown-model");
    after.timestamp = at(1, 0) + Duration::seconds(45);
    let first = point(1, 0.0, reset);
    let sample = point(2, 1.0, reset + Duration::seconds(10));
    let result = attribute(
        &[before, after],
        &[],
        &BTreeMap::new(),
        &[first, sample],
        &[],
        &Pricing::bundled().expect("pricing"),
    );
    approx(result.events[0].five_hour_delta, 0.0);
    approx(result.events[1].five_hour_delta, 1.0);
}

#[test]
fn attribution_covers_unpriced_tokens_empty_intervals_resets_and_decreases() {
    let mut next_event = event(8, 100, "unknown-model");
    next_event.timestamp = at(5, 8);
    let events = vec![
        event(1, 100, "unknown-model"),
        event(2, 300, "unknown-model"),
        next_event,
    ];
    let mut next_point = point(9, 2.0, at(10, 0));
    next_point.at = at(5, 9);
    let points = vec![
        point(3, 4.0, at(5, 0)),
        point(4, 3.0, at(5, 0)),
        point(5, 5.0, at(5, 0)),
        next_point,
    ];
    let result = attribute(
        &events,
        &[],
        &BTreeMap::new(),
        &points,
        &[],
        &Pricing::bundled().expect("pricing"),
    );
    approx(result.events[0].five_hour_delta, 1.0);
    approx(result.events[1].five_hour_delta, 3.0);
    approx(result.events[2].five_hour_delta, 2.0);
    let first = result
        .windows
        .iter()
        .find(|w| w.window_id == at(5, 0))
        .expect("first window");
    approx(first.increment_total, 6.0);
    approx(first.unattributed_total, 2.0);
    assert_eq!(first.decreases, 1);
    let second = result
        .windows
        .iter()
        .find(|w| w.window_id == at(10, 0))
        .expect("next window");
    approx(second.increment_total, 2.0);
    // A positive sample with no local request remains explicitly unattributed.
    let empty = attribute(
        &[],
        &[],
        &BTreeMap::new(),
        &[point(2, 2.0, at(5, 0))],
        &[],
        &Pricing::bundled().expect("pricing"),
    );
    approx(empty.windows[0].unattributed_total, 2.0);
}

#[test]
fn forecast_returns_all_three_states() {
    let points = vec![point(1, 40.0, at(3, 0)), point(21, 80.0, at(3, 0))];
    assert!(matches!(
        forecast(&points, Tool::Claude, ForecastKind::FiveHour, at(1, 22)),
        Forecast::WillHit { .. }
    ));
    assert_eq!(
        forecast(
            &[point(1, 10.0, at(5, 0)), point(21, 10.0, at(5, 0))],
            Tool::Claude,
            ForecastKind::FiveHour,
            at(1, 22)
        ),
        Forecast::Ample
    );
    assert_eq!(
        forecast(&[], Tool::Claude, ForecastKind::FiveHour, at(1, 22)),
        Forecast::Unknown
    );
}

#[test]
fn idle_time_reduces_forecast_and_passed_window_has_no_usage() {
    let points = vec![point(1, 0.0, at(5, 0)), point(2, 33.0, at(5, 0))];
    assert_eq!(
        forecast(&points, Tool::Claude, ForecastKind::FiveHour, at(2, 33)),
        Forecast::Ample
    );
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(&[], &[], &BTreeMap::new(), &points, &[], &pricing);
    let overview = build_overview(
        &OverviewInput {
            events: &[],
            titles: &BTreeMap::new(),
            points: &points,
            attribution: &attribution,
            claude_snapshots: &[],
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(5, 1),
        Utc,
    );
    assert!(overview.tools[0].five_hour.window_passed);
    assert_eq!(overview.tools[0].five_hour.used_percent, None);
    assert_eq!(overview.tools[0].five_hour.forecast, Forecast::Unknown);
}

#[test]
fn menu_bar_tools_default_empty_ignore_legacy_and_cap_at_two() {
    let temp = Temp::new();
    // Old files carry `menu_bar_quota`; it no longer means anything and must not fail.
    fs::write(
        temp.path().join("settings.json"),
        b"{\"menu_bar_quota\":\"both\",\"theme\":\"dark\"}",
    )
    .expect("legacy settings");
    let mut settings = read_settings(temp.path()).expect("legacy settings");
    assert_eq!(settings.theme, Theme::Dark);
    assert!(settings.menu_bar_tools.is_empty());
    settings.menu_bar_tools = vec![Tool::Codex, Tool::Codex, Tool::Claude, Tool::Claude];
    write_settings(temp.path(), &settings).expect("save settings");
    assert_eq!(
        read_settings(temp.path()).expect("saved").menu_bar_tools,
        vec![Tool::Codex, Tool::Claude]
    );
    let mut tools = vec![Tool::Claude];
    toggle_menu_bar_tool(&mut tools, Tool::Codex);
    assert_eq!(tools, vec![Tool::Claude, Tool::Codex]);
    toggle_menu_bar_tool(&mut tools, Tool::Claude);
    assert_eq!(tools, vec![Tool::Codex]);
}

#[test]
fn settings_and_quota_store_round_trip_and_skip_bad_lines() {
    let temp = Temp::new();
    assert_eq!(
        read_settings(temp.path()).expect("default settings"),
        AppSettings::default()
    );
    fs::write(temp.path().join("settings.json"), b"{\"theme\":\"dark\"}")
        .expect("partial settings");
    let mut settings = read_settings(temp.path()).expect("partial settings");
    assert_eq!(settings.theme, Theme::Dark);
    assert!(!settings.claude_official_query);
    settings.onboarding_completed = true;
    write_settings(temp.path(), &settings).expect("save settings");
    assert_eq!(
        read_settings(temp.path()).expect("saved settings"),
        settings
    );
    let snapshot = ClaudeSnapshot {
        at: at(1, 0),
        subscription_type: Some("pro".into()),
        rate_limit_tier: None,
        five_hour: Some(StoredWindow {
            used_percent: 8.0,
            resets_at: Some(at(5, 0)),
            window_minutes: Some(300),
        }),
        seven_day: None,
        weekly_breakdown: None,
    };
    append_claude(temp.path(), &snapshot).expect("append snapshot");
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(temp.path().join("quota/claude.jsonl"))
        .expect("open quota fixture")
        .write_all(b"broken\n")
        .expect("write malformed line");
    let (read, stats) = read_claude(temp.path());
    assert_eq!(read, vec![snapshot]);
    assert_eq!(stats.malformed_lines, 1);
}

#[test]
fn overview_uses_fixed_local_day_and_attribution() {
    let events = vec![
        event(1, 100, "claude-opus-5"),
        event(2, 200, "claude-opus-5"),
    ];
    let points = vec![point(3, 6.0, at(5, 0))];
    let pricing = Pricing::bundled().expect("pricing");
    let titles = BTreeMap::from([("session-a".into(), "会话标题".into())]);
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "session-a".into(),
        timestamp: at(1, 0),
    }];
    let attribution = attribute(&events, &prompts, &titles, &points, &[], &pricing);
    let overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &[],
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(2, 0),
        Utc,
    );
    assert_eq!(overview.today_tokens.input, 300);
    assert_eq!(overview.hourly_tokens[1], 300);
    assert_eq!(overview.peak_hour, Some(1));
    assert_eq!(overview.summary.sessions, 1);
    assert_eq!(overview.summary.requests, 2);
    assert_eq!(overview.summary.high_prompts, 1);
    approx(overview.recent_sessions[0].five_hour_delta, 6.0);
    assert_eq!(
        overview.recent_sessions[0].title.as_deref(),
        Some("会话标题")
    );
    assert_eq!(overview.tools[0].five_hour.origin, Some(Origin::Official));
    assert_eq!(overview.tools[1].five_hour.origin, Some(Origin::Estimated));
    assert_eq!(overview.trends[0].high_prompts.len(), 1);
    assert_eq!(overview.trends[0].today.len(), 1);
    assert_eq!(overview.daily_cache_hit_rate.len(), 14);
    assert_eq!(
        overview
            .daily_cache_hit_rate
            .last()
            .and_then(|day| day.rate),
        overview.today_cache_hit_rate
    );
    assert_eq!(overview.subscription_total_usd, None);
    assert_eq!(overview.subscription_value_multiple, None);
    assert_eq!(overview.recent_high_prompts.len(), 1);
    assert_eq!(overview.recent_high_prompts[0].source, Source::ClaudeCli);
    assert!(overview.month_estimated_cost_usd > 0.0);
    assert_eq!(overview.daily_cost.len(), 24);
    assert!(overview.today_cache_hit_rate.is_some());
    assert!(overview.trends[0].five_hour[0].percent == 6.0);
    assert_eq!(attribution.windows[0].kind, WindowKind::FiveHour);
}

#[test]
fn subscription_multiple_counts_only_tools_with_recognized_plans() {
    let mut claude = event(1, 1_000_000, "claude-opus-5");
    claude.session_id = "claude-sample".into();
    let mut codex = event(2, 1_000_000, "gpt-6-sol");
    codex.tool = Tool::Codex;
    codex.source = Source::CodexCli;
    codex.session_id = "codex-sample".into();
    let events = vec![claude, codex];
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(&events, &[], &BTreeMap::new(), &[], &[], &pricing);
    let codex_snapshots = vec![CodexSnapshot {
        at: at(1, 3),
        limit_id: None,
        plan_type: Some("plus".into()),
        primary: None,
        secondary: None,
        reset_credits_available: None,
    }];
    let build = |claude_snapshots: &[ClaudeSnapshot], codex_snapshots: &[CodexSnapshot]| {
        build_overview(
            &OverviewInput {
                events: &events,
                titles: &BTreeMap::new(),
                points: &[],
                attribution: &attribution,
                claude_snapshots,
                codex_snapshots,
                codex_log: &[],
                pricing: &pricing,
                recorder: RecorderStatus::default(),
            },
            at(2, 0),
            Utc,
        )
    };
    let codex_only = build(&[], &codex_snapshots);
    let claude_cost = codex_only.month_cost_by_tool[&Tool::Claude];
    let codex_cost = codex_only.month_cost_by_tool[&Tool::Codex];
    assert!(claude_cost > 0.0 && codex_cost > 0.0);
    approx(
        codex_only.month_estimated_cost_usd,
        claude_cost + codex_cost,
    );
    assert_eq!(codex_only.subscription_total_usd, Some(20.0));
    approx(
        codex_only.subscription_value_multiple.expect("known plan"),
        codex_cost / 20.0,
    );

    let unknown = build(&[], &[]);
    assert_eq!(unknown.subscription_total_usd, None);
    assert_eq!(unknown.subscription_value_multiple, None);

    let claude_snapshots = vec![ClaudeSnapshot {
        at: at(1, 3),
        subscription_type: Some("pro".into()),
        rate_limit_tier: None,
        five_hour: None,
        seven_day: None,
        weekly_breakdown: None,
    }];
    let both = build(&claude_snapshots, &codex_snapshots);
    assert_eq!(both.subscription_total_usd, Some(40.0));
    approx(
        both.subscription_value_multiple.expect("both plans"),
        (claude_cost + codex_cost) / 40.0,
    );
}

#[test]
fn month_cost_map_retains_usage_without_a_model_price() {
    let events = vec![event(1, 100, "unpriced-handwritten-model")];
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(&events, &[], &BTreeMap::new(), &[], &[], &pricing);
    let overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &BTreeMap::new(),
            points: &[],
            attribution: &attribution,
            claude_snapshots: &[],
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(2, 0),
        Utc,
    );
    assert_eq!(overview.month_cost_by_tool.get(&Tool::Claude), Some(&0.0));
    assert!(!overview.month_cost_by_tool.contains_key(&Tool::Codex));
}

#[test]
fn overview_uses_current_five_hour_and_weekly_prompt_thresholds() {
    let weekly_reset = at(4, 0) + Duration::days(7);
    let make_point = |hour, minute, five, reset, weekly| QuotaPoint {
        tool: Tool::Claude,
        at: at(hour, minute),
        origin: Origin::Official,
        five_hour: Some(Window {
            used_percent: five,
            resets_at: Some(reset),
            window_minutes: Some(300),
        }),
        weekly: Some(Window {
            used_percent: weekly,
            resets_at: Some(weekly_reset),
            window_minutes: Some(10080),
        }),
    };
    let make_event = |hour, minute, second, session: &str, model: &str, input| {
        let mut e = event(0, input, model);
        e.timestamp = at(hour, minute) + Duration::seconds(second);
        e.session_id = session.into();
        e
    };
    let events = vec![
        make_event(6, 1, 0, "a", "claude-opus-5", 100),
        make_event(6, 2, 0, "d", "claude-opus-5", 100),
        make_event(10, 1, 0, "a", "claude-opus-5", 100),
        make_event(10, 3, 30, "b", "claude-haiku-5-5", 10),
        make_event(10, 4, 30, "c", "claude-opus-5", 1000),
    ];
    let points = vec![
        make_point(6, 3, 40.0, at(9, 0), 0.4),
        make_point(10, 3, 0.6, at(14, 0), 1.0),
        make_point(10, 4, 1.6, at(14, 0), 1.4),
        make_point(10, 5, 1.8, at(14, 0), 1.8),
    ];
    let prompts = vec![
        PromptEvent {
            tool: Tool::Claude,
            session_id: "a".into(),
            timestamp: at(6, 0),
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "d".into(),
            timestamp: at(6, 0),
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "a".into(),
            timestamp: at(10, 0),
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "b".into(),
            timestamp: at(10, 3),
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "c".into(),
            timestamp: at(10, 4),
        },
    ];
    let pricing = Pricing::bundled().expect("pricing");
    let titles = BTreeMap::new();
    let attribution = attribute(&events, &prompts, &titles, &points, &[], &pricing);
    let overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &[],
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(12, 0),
        Utc,
    );
    assert_eq!(overview.summary.high_prompts, 2);
    assert_eq!(overview.trends[0].today.len(), 4);
    assert_eq!(overview.trends[0].week.len(), 4);
    assert_eq!(overview.trends[0].week_high_prompts.len(), 0);
    assert_eq!(overview.recent_high_prompts.len(), 2);
    assert!(
        overview.recent_high_prompts[0].weekly_delta
            >= overview.recent_high_prompts[1].weekly_delta
    );
    assert_eq!(
        overview
            .recent_sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "c", "b", "d"]
    );
    approx(overview.recent_sessions[0].five_hour_delta, 0.6);
    approx(overview.recent_sessions[0].weekly_delta, 0.8);
    approx(overview.recent_sessions[1].weekly_delta, 0.4);
    approx(overview.recent_sessions[2].weekly_delta, 0.4);
    approx(overview.recent_sessions[3].five_hour_delta, 0.0);
}

#[test]
fn overview_prices_known_plans_and_marks_weekly_prompt() {
    let mut first = point(0, 0.0, at(6, 0));
    first.weekly = Some(Window {
        used_percent: 0.0,
        resets_at: Some(at(0, 0) + Duration::days(7)),
        window_minutes: Some(10080),
    });
    let mut second = point(3, 3.0, at(6, 0));
    second.weekly = Some(Window {
        used_percent: 2.0,
        resets_at: Some(at(0, 0) + Duration::days(7)),
        window_minutes: Some(10080),
    });
    let points = vec![first, second];
    let events = vec![event(1, 100, "claude-opus-5")];
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "session-a".into(),
        timestamp: at(1, 0),
    }];
    let pricing = Pricing::bundled().expect("pricing");
    let titles = BTreeMap::from([("session-a".into(), "手写样例".into())]);
    let attribution = attribute(&events, &prompts, &titles, &points, &[], &pricing);
    let claude_snapshots = vec![ClaudeSnapshot {
        at: at(1, 0),
        subscription_type: Some("pro".into()),
        rate_limit_tier: None,
        five_hour: None,
        seven_day: None,
        weekly_breakdown: None,
    }];
    let overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &claude_snapshots,
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(2, 0),
        Utc,
    );
    assert_eq!(overview.tools[0].subscription_usd, Some(20.0));
    assert_eq!(overview.tools[1].subscription_usd, None);
    assert_eq!(overview.subscription_total_usd, Some(20.0));
    assert_eq!(
        overview.subscription_value_multiple,
        Some(overview.month_estimated_cost_usd / 20.0)
    );
    assert_eq!(overview.today_token_total, 100);
    assert_eq!(overview.recent_sessions[0].total_tokens, 100);
    assert_eq!(overview.trends[0].week_high_prompts.len(), 1);
    assert_eq!(overview.trends[0].today_high_prompts.len(), 1);
    assert_eq!(overview.recent_high_prompts[0].total_tokens, 100);
    let mut max_snapshots = claude_snapshots;
    max_snapshots[0].subscription_type = Some("max".into());
    max_snapshots[0].rate_limit_tier = Some("default_claude_max_5x".into());
    let max_overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &max_snapshots,
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(2, 0),
        Utc,
    );
    assert_eq!(max_overview.tools[0].plan.as_deref(), Some("max_5x"));
    assert_eq!(max_overview.tools[0].subscription_usd, Some(100.0));
    max_snapshots[0].rate_limit_tier = None;
    let unknown_overview = build_overview(
        &OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &max_snapshots,
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        at(2, 0),
        Utc,
    );
    assert_eq!(unknown_overview.tools[0].plan, None);
    assert_eq!(unknown_overview.tools[0].subscription_usd, None);
}
