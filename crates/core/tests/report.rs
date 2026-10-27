use chrono::{DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use token_audit_core::model::{Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::report::{build_report, ReportOptions};

fn event(
    timestamp: &str,
    tool: Tool,
    source: Source,
    model: &str,
    session: &str,
    context: (Option<&str>, bool),
    tokens: TokenCounts,
) -> UsageEvent {
    UsageEvent {
        tool,
        source,
        session_id: session.into(),
        request_id: None,
        timestamp: DateTime::parse_from_rfc3339(timestamp)
            .expect("timestamp")
            .with_timezone(&Utc),
        model: model.into(),
        project: context.0.map(str::to_owned),
        is_subagent: context.1,
        fast: false,
        effort: None,
        web_search_requests: 0,
        tokens,
    }
}

fn sample() -> Vec<UsageEvent> {
    vec![
        event(
            "2026-10-23T23:30:00Z",
            Tool::Claude,
            Source::ClaudeCli,
            "claude-opus-5-5",
            "session-a",
            (Some("/work/alpha"), false),
            TokenCounts {
                input: 1_000_000,
                output: 2_000_000,
                cache_read: 500_000,
                cache_write_5m: 100_000,
                cache_write_1h: 200_000,
                cache_write_unspecified: 300_000,
                ..TokenCounts::default()
            },
        ),
        event(
            "2026-10-24T01:00:00Z",
            Tool::Codex,
            Source::CodexDesktop,
            "gpt-6.1-sol",
            "session-b",
            (Some("/work/beta"), true),
            TokenCounts {
                input: 1_000_000,
                output: 1_000_000,
                cache_read: 500_000,
                cache_write_unspecified: 200_000,
                reasoning: 900_000,
                ..TokenCounts::default()
            },
        ),
        event(
            "2026-10-24T23:30:00Z",
            Tool::Claude,
            Source::Unknown,
            "unknown-model",
            "session-a",
            (Some("/work/alpha"), true),
            TokenCounts {
                output: 10,
                ..TokenCounts::default()
            },
        ),
    ]
}

fn options() -> ReportOptions<FixedOffset> {
    ReportOptions {
        since: None,
        until: None,
        tool: None,
        tz: FixedOffset::east_opt(8 * 3600).expect("+08:00"),
    }
}

#[derive(Clone)]
struct SpringForward;

impl SpringForward {
    fn transition() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .expect("transition date")
            .and_hms_opt(1, 0, 0)
            .expect("transition time")
    }

    fn before() -> FixedOffset {
        FixedOffset::east_opt(0).expect("UTC offset")
    }

    fn after() -> FixedOffset {
        FixedOffset::east_opt(3600).expect("summer offset")
    }
}

impl TimeZone for SpringForward {
    type Offset = FixedOffset;

    fn from_offset(_: &Self::Offset) -> Self {
        Self
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
        self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).expect("midnight"))
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<Self::Offset> {
        let gap_start = Self::transition();
        let gap_end = gap_start + chrono::Duration::hours(1);
        if *local < gap_start {
            MappedLocalTime::Single(Self::before())
        } else if *local < gap_end {
            MappedLocalTime::None
        } else {
            MappedLocalTime::Single(Self::after())
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).expect("midnight"))
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        if *utc < Self::transition() {
            Self::before()
        } else {
            Self::after()
        }
    }
}

#[test]
fn daylight_saving_offsets_apply_per_event_and_date_filter() {
    let tokens = TokenCounts {
        output: 1,
        ..TokenCounts::default()
    };
    let events = [
        event(
            "2026-03-28T23:30:00Z",
            Tool::Claude,
            Source::ClaudeCli,
            "unknown",
            "a",
            (None, false),
            tokens,
        ),
        event(
            "2026-03-29T00:30:00Z",
            Tool::Claude,
            Source::ClaudeCli,
            "unknown",
            "b",
            (None, false),
            tokens,
        ),
        event(
            "2026-03-29T23:30:00Z",
            Tool::Claude,
            Source::ClaudeCli,
            "unknown",
            "c",
            (None, false),
            tokens,
        ),
    ];
    let pricing = Pricing::bundled().expect("prices");
    let mut options = ReportOptions {
        since: None,
        until: None,
        tool: None,
        tz: SpringForward,
    };
    let report = build_report(&events, &pricing, &options);
    let days: Vec<_> = report
        .by_day
        .iter()
        .map(|group| group.key.to_string())
        .collect();
    assert_eq!(days, ["2026-03-28", "2026-03-29", "2026-03-30"]);

    let selected_day = NaiveDate::from_ymd_opt(2026, 3, 29).expect("selected day");
    options.since = Some(selected_day);
    options.until = Some(selected_day);
    let filtered = build_report(&events, &pricing, &options);
    assert_eq!(filtered.total.events, 1);
    assert_eq!(filtered.by_day[0].key, selected_day);
}

#[test]
fn totals_cost_hit_rate_unpriced_and_sessions() {
    let report = build_report(&sample(), &Pricing::bundled().expect("prices"), &options());
    // Claude 47.7; Codex 2 + 10 + 0.05 + 0.5 = 12.55.
    assert!((report.total.estimated_cost_usd - 60.25).abs() < 1e-9);
    assert_eq!(report.total.events, 3);
    assert_eq!(report.total.unpriced_events, 1);
    assert_eq!(report.total.unpriced_tokens.output, 10);
    assert!((report.total.cache_hit_rate.expect("hit rate") - 1.0 / 3.8).abs() < 1e-9);
    assert_eq!(report.by_day.len(), 2);
    assert_eq!(report.by_day[0].key.to_string(), "2026-10-24");
    assert_eq!(report.by_day[0].metrics.events, 2);
    assert_eq!(report.by_day[1].key.to_string(), "2026-10-25");
    assert_eq!(report.by_day[1].metrics.cache_hit_rate, None);
    assert_eq!(report.by_session[0].key, "session-a");
    assert_eq!(
        report.by_session[0].models,
        vec!["claude-opus-5-5", "unknown-model"]
    );
    assert_eq!(
        report.by_session[0].first_timestamp.to_rfc3339(),
        "2026-10-23T23:30:00+00:00"
    );
    assert_eq!(
        report.by_session[0].last_timestamp.to_rfc3339(),
        "2026-10-24T23:30:00+00:00"
    );
    assert!(!report.by_subagent[0].key);
    assert!(report.by_subagent[1].key);
    assert_eq!(report.by_subagent[1].metrics.events, 2);
    assert_eq!(report.by_model[0].key, "claude-opus-5-5");
    assert_eq!(report.by_project[0].key.as_deref(), Some("/work/alpha"));
}

#[test]
fn inclusive_local_date_bounds_and_tool_filter() {
    let events = sample();
    let pricing = Pricing::bundled().expect("prices");
    let mut opts = options();
    let day = NaiveDate::from_ymd_opt(2026, 10, 24).expect("day");
    opts.since = Some(day);
    opts.until = Some(day);
    let report = build_report(&events, &pricing, &opts);
    assert_eq!(report.total.events, 2);
    assert_eq!(report.by_day.len(), 1);
    opts.tool = Some(Tool::Claude);
    let report = build_report(&events, &pricing, &opts);
    assert_eq!(report.total.events, 1);
    assert_eq!(report.by_tool[0].key, Tool::Claude);
    assert_eq!(report.total.unpriced_events, 0);
}

#[test]
fn equal_cost_groups_sort_by_key_and_repeat_identically() {
    let mut events = sample();
    events.push(event(
        "2026-10-25T00:00:00Z",
        Tool::Claude,
        Source::Unknown,
        "unknown-z",
        "session-z",
        (None, false),
        TokenCounts {
            output: 1,
            ..TokenCounts::default()
        },
    ));
    let pricing = Pricing::bundled().expect("prices");
    let first = serde_json::to_value(build_report(&events, &pricing, &options())).expect("json");
    for _ in 0..3 {
        assert_eq!(
            serde_json::to_value(build_report(&events, &pricing, &options())).expect("json"),
            first
        );
    }
    let report = build_report(&events, &pricing, &options());
    assert_eq!(report.by_model[2].key, "unknown-model");
    assert_eq!(report.by_model[3].key, "unknown-z");
}
