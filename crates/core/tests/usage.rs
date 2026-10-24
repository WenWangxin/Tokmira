use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use token_audit_core::attribution::{attribute, WindowKind};
use token_audit_core::claude_quota::{BreakdownRow, WeeklyBreakdown};
use token_audit_core::dashboard::OverviewInput;
use token_audit_core::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::ClaudeSnapshot;
use token_audit_core::recorder::RecorderStatus;
use token_audit_core::timeline::{Origin, QuotaPoint, Window};
use token_audit_core::usage::{list_usage_windows, usage_detail};

#[derive(Deserialize)]
struct PointRow {
    at: DateTime<Utc>,
    five_reset: DateTime<Utc>,
    five_used: f64,
    week_used: f64,
}
#[derive(Deserialize)]
struct EventRow {
    at: DateTime<Utc>,
    session: String,
    project: String,
    tokens: u64,
}
#[derive(Deserialize)]
struct Fixture {
    points: Vec<PointRow>,
    events: Vec<EventRow>,
}

#[test]
fn windows_and_detail_use_only_the_selected_range() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/usage_synthetic.json")).unwrap();
    let now = "2026-10-24T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let week_reset = "2026-10-29T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let week_start = "2026-10-22T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let points: Vec<_> = fixture
        .points
        .into_iter()
        .map(|row| QuotaPoint {
            tool: Tool::Claude,
            at: row.at,
            origin: Origin::Statusline,
            five_hour: Some(Window {
                used_percent: row.five_used,
                resets_at: Some(row.five_reset),
                window_minutes: Some(300),
            }),
            weekly: Some(Window {
                used_percent: row.week_used,
                resets_at: Some(week_reset),
                window_minutes: Some(10080),
            }),
        })
        .collect();
    let events: Vec<_> = fixture
        .events
        .into_iter()
        .map(|row| UsageEvent {
            tool: Tool::Claude,
            source: Source::ClaudeCli,
            session_id: row.session,
            request_id: None,
            timestamp: row.at,
            model: "synthetic-model".to_owned(),
            project: Some(row.project),
            is_subagent: false,
            tokens: TokenCounts {
                input: row.tokens,
                ..Default::default()
            },
        })
        .collect();
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "current-session".to_owned(),
        timestamp: "2026-10-24T11:15:00Z".parse().unwrap(),
    }];
    let titles = BTreeMap::from([("current-session".to_owned(), "合成测试会话".to_owned())]);
    let snapshots = vec![ClaudeSnapshot {
        at: now,
        subscription_type: None,
        five_hour: None,
        seven_day: None,
        weekly_breakdown: Some(WeeklyBreakdown {
            as_of: Some(now),
            window_started_at: Some(week_start),
            rows: vec![BreakdownRow {
                key: "claude_code".to_owned(),
                display_name: Some("Claude Code".to_owned()),
                percent: 98.0,
            }],
        }),
    }];
    let pricing = Pricing::bundled().unwrap();
    let attribution = attribute(&events, &prompts, &titles, &points, &snapshots, &pricing);
    let input = OverviewInput {
        events: &events,
        titles: &titles,
        points: &points,
        attribution: &attribution,
        claude_snapshots: &snapshots,
        codex_snapshots: &[],
        codex_log: &[],
        pricing: &pricing,
        recorder: RecorderStatus::default(),
    };
    let windows = list_usage_windows(&input, Tool::Claude, now, 6);
    assert_eq!(windows.len(), 3);
    assert_eq!(windows[0].kind, WindowKind::FiveHour);
    assert!(windows[0].is_current);
    assert_eq!(windows[0].start.to_rfc3339(), "2026-10-24T10:00:00+00:00");
    assert_eq!(windows[0].end.to_rfc3339(), "2026-10-24T15:00:00+00:00");
    assert!(!windows[1].is_current);
    assert_eq!(windows[1].end.to_rfc3339(), "2026-10-24T10:00:00+00:00");
    assert_eq!(windows[2].kind, WindowKind::Weekly);
    assert!(windows[2].is_current);
    assert_eq!(list_usage_windows(&input, Tool::Claude, now, 1).len(), 2);
    let current = usage_detail(&input, &windows[0], now);
    assert_eq!(current.used_percent, 10.0);
    assert_eq!(current.remaining_percent, 90.0);
    assert_eq!(current.origin, Origin::Statusline);
    assert_eq!(current.sessions.len(), 1);
    assert_eq!(current.sessions[0].name, "合成测试会话");
    assert_eq!(current.sessions[0].tokens, 200);
    assert_eq!(current.sessions[0].requests, 1);
    assert!((current.attributed_total - 6.0).abs() < 0.0001);
    assert!((current.unattributed_total - 4.0).abs() < 0.0001);
    assert_eq!(current.high_prompts.len(), 1);
    assert!(current.official_weekly_breakdown.is_none());
    let weekly = usage_detail(&input, &windows[2], now);
    assert_eq!(
        weekly.official_weekly_breakdown.unwrap().rows[0].percent,
        98.0
    );
    let prior = usage_detail(&input, &windows[1], now);
    assert_eq!(prior.sessions[0].tokens, 100);
}

#[test]
fn quota_point_without_local_requests_is_unattributed() {
    let at: DateTime<Utc> = "2026-10-24T12:00:00Z".parse().unwrap();
    let reset: DateTime<Utc> = "2026-10-24T15:00:00Z".parse().unwrap();
    let points = vec![QuotaPoint {
        tool: Tool::Codex,
        at,
        origin: Origin::Log,
        five_hour: Some(Window {
            used_percent: 12.0,
            resets_at: Some(reset),
            window_minutes: Some(300),
        }),
        weekly: None,
    }];
    let pricing = Pricing::bundled().unwrap();
    let attribution = attribute(&[], &[], &BTreeMap::new(), &points, &[], &pricing);
    let input = OverviewInput {
        events: &[],
        titles: &BTreeMap::new(),
        points: &points,
        attribution: &attribution,
        claude_snapshots: &[],
        codex_snapshots: &[],
        codex_log: &[],
        pricing: &pricing,
        recorder: RecorderStatus::default(),
    };
    let window = list_usage_windows(&input, Tool::Codex, at, 6).remove(0);
    let detail = usage_detail(&input, &window, at);
    assert!(detail.sessions.is_empty());
    assert!(detail.high_prompts.is_empty());
    assert_eq!(detail.attributed_total, 0.0);
    assert_eq!(detail.unattributed_total, 12.0);
}
