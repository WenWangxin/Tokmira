use std::collections::BTreeMap;

use chrono::{DateTime, TimeZone, Utc};
use token_audit_core::attribution::{attribute, WindowKind};
use token_audit_core::dashboard::{build_overview, OverviewInput, QuotaDisplay};
use token_audit_core::estimate::{cost_since, learn_rate, window_rate};
use token_audit_core::model::{Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::recorder::RecorderStatus;
use token_audit_core::timeline::{Origin, QuotaPoint, Window};

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, hour, minute, 0)
        .single()
        .expect("fixed time")
}
/// One request on claude-opus-5-5 costing `dollars` (input is $4 per million tokens).
fn request(hour: u32, minute: u32, dollars: f64) -> UsageEvent {
    UsageEvent {
        tool: Tool::Claude,
        source: Source::ClaudeDesktop,
        session_id: "session".into(),
        request_id: Some(format!("request-{hour}-{minute}")),
        timestamp: at(hour, minute),
        model: "claude-opus-5-5".into(),
        project: None,
        is_subagent: false,
        fast: false,
        effort: None,
        web_search_requests: 0,
        tokens: TokenCounts {
            input: (dollars * 250_000.0) as u64,
            ..TokenCounts::default()
        },
    }
}
fn reading(minute: u32, percent: f64) -> QuotaPoint {
    QuotaPoint {
        tool: Tool::Claude,
        at: at(1, minute),
        origin: Origin::Official,
        five_hour: Some(Window {
            used_percent: percent,
            resets_at: Some(at(5, 0)),
            window_minutes: Some(300),
        }),
        weekly: None,
    }
}
fn approx(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

/// Readings of 10% at 01:00 and 40% at 01:30 with six $1 requests in between: 5% per dollar.
fn history() -> (Vec<UsageEvent>, Vec<QuotaPoint>) {
    let events = (0..6).map(|i| request(1, 5 + i * 4, 1.0)).collect();
    (events, vec![reading(0, 10.0), reading(30, 40.0)])
}

fn five_hour(events: &[UsageEvent], points: &[QuotaPoint], now: DateTime<Utc>) -> QuotaDisplay {
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(events, &[], &BTreeMap::new(), points, &[], &pricing);
    let overview = build_overview(
        &OverviewInput {
            events,
            titles: &BTreeMap::new(),
            points,
            attribution: &attribution,
            claude_snapshots: &[],
            codex_snapshots: &[],
            codex_log: &[],
            pricing: &pricing,
            recorder: RecorderStatus::default(),
        },
        now,
        Utc,
    );
    overview.tools[0].five_hour.clone()
}

#[test]
fn the_rate_comes_from_requests_that_shared_an_increase() {
    let (events, points) = history();
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(&events, &[], &BTreeMap::new(), &points, &[], &pricing);
    let rate = learn_rate(
        &attribution.events,
        &pricing,
        Tool::Claude,
        WindowKind::FiveHour,
        at(1, 30),
    )
    .expect("enough history");
    approx(rate, 5.0);
    // Codex has no history here, and the weekly window had no readings.
    assert!(learn_rate(
        &attribution.events,
        &pricing,
        Tool::Codex,
        WindowKind::FiveHour,
        at(1, 30)
    )
    .is_none());
    assert!(learn_rate(
        &attribution.events,
        &pricing,
        Tool::Claude,
        WindowKind::Weekly,
        at(1, 30)
    )
    .is_none());
}

#[test]
fn too_little_history_gives_no_rate() {
    let events: Vec<_> = (0..3).map(|i| request(1, 5 + i * 4, 1.0)).collect();
    let points = vec![reading(0, 10.0), reading(30, 40.0)];
    let pricing = Pricing::bundled().expect("pricing");
    let attribution = attribute(&events, &[], &BTreeMap::new(), &points, &[], &pricing);
    assert!(learn_rate(
        &attribution.events,
        &pricing,
        Tool::Claude,
        WindowKind::FiveHour,
        at(1, 30)
    )
    .is_none());
}

#[test]
fn cost_since_counts_only_later_requests_of_the_tool() {
    let pricing = Pricing::bundled().expect("pricing");
    let events = vec![
        request(1, 10, 1.0),
        request(1, 40, 2.0),
        request(1, 50, 0.5),
    ];
    approx(
        cost_since(&events, &pricing, Tool::Claude, at(1, 30), at(1, 45)),
        2.0,
    );
    approx(
        cost_since(&events, &pricing, Tool::Codex, at(1, 30), at(1, 59)),
        0.0,
    );
}

#[test]
fn requests_after_the_latest_reading_are_added_and_marked() {
    let (mut events, points) = history();
    events.push(request(1, 40, 1.0));
    let display = five_hour(&events, &points, at(1, 45));
    // The window began at 00:00 and saw $6 by the 40% reading: 40 / 6 per dollar.
    approx(display.used_percent.expect("percent"), 40.0 + 40.0 / 6.0);
    approx(
        display.estimated_percent.expect("estimated part"),
        40.0 / 6.0,
    );
    assert_eq!(display.as_of, Some(at(1, 30)));
    assert_eq!(display.origin, Some(Origin::Official));
}

#[test]
fn the_window_rate_needs_enough_spend_in_the_window() {
    let pricing = Pricing::bundled().expect("pricing");
    let (events, _) = history();
    let rate = window_rate(&events, &pricing, Tool::Claude, 40.0, at(0, 0), at(1, 30));
    approx(rate.expect("$6 in the window"), 40.0 / 6.0);
    assert!(window_rate(&events, &pricing, Tool::Claude, 40.0, at(1, 26), at(1, 30)).is_none());
    assert!(window_rate(&events, &pricing, Tool::Claude, 0.0, at(0, 0), at(1, 30)).is_none());
}

#[test]
fn a_new_window_without_spend_falls_back_to_the_learned_rate() {
    let (mut events, mut points) = history();
    // The next window (05:00–10:00) reads 2% at 05:30 before any local request in it.
    points.push(QuotaPoint {
        tool: Tool::Claude,
        at: at(5, 30),
        origin: Origin::Official,
        five_hour: Some(Window {
            used_percent: 2.0,
            resets_at: Some(at(10, 0)),
            window_minutes: Some(300),
        }),
        weekly: None,
    });
    events.push(request(5, 40, 1.0));
    let display = five_hour(&events, &points, at(5, 45));
    // Two weeks of attribution: 30 points over $6 between the first window's readings.
    approx(display.estimated_percent.expect("estimated part"), 5.0);
    approx(display.used_percent.expect("percent"), 7.0);
}

#[test]
fn nothing_is_estimated_without_new_requests() {
    let (events, points) = history();
    let display = five_hour(&events, &points, at(1, 45));
    approx(display.used_percent.expect("percent"), 40.0);
    assert_eq!(display.estimated_percent, None);
}

#[test]
fn the_estimate_stops_at_the_limit() {
    let (mut events, points) = history();
    events.push(request(1, 40, 50.0));
    let display = five_hour(&events, &points, at(1, 45));
    approx(display.used_percent.expect("percent"), 100.0);
    approx(display.estimated_percent.expect("estimated part"), 60.0);
}

#[test]
fn a_reset_window_shows_no_estimate() {
    let (mut events, points) = history();
    events.push(request(5, 10, 1.0));
    let display = five_hour(&events, &points, at(5, 20));
    assert!(display.window_passed);
    assert_eq!(display.used_percent, None);
    assert_eq!(display.estimated_percent, None);
}
