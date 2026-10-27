use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use token_audit_core::advice::{EventTag, Severity};
use token_audit_core::attribution::EventAttribution;
use token_audit_core::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::sessions::{list_sessions, session_detail, SessionInput, TimelineEntry};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0)
        .single()
        .expect("fixed time")
}

fn at(minutes_ago: i64) -> DateTime<Utc> {
    now() - Duration::minutes(minutes_ago)
}

fn tokens(input: u64, cache_read: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input,
        cache_read,
        output,
        ..TokenCounts::default()
    }
}

fn event(
    tool: Tool,
    session: &str,
    model: &str,
    project: Option<&str>,
    minutes_ago: i64,
    counts: TokenCounts,
    weekly_delta: f64,
) -> EventAttribution {
    EventAttribution {
        event: UsageEvent {
            tool,
            source: match tool {
                Tool::Claude => Source::ClaudeCli,
                Tool::Codex => Source::CodexCli,
            },
            session_id: session.to_owned(),
            request_id: None,
            timestamp: at(minutes_ago),
            model: model.to_owned(),
            project: project.map(str::to_owned),
            is_subagent: false,
            fast: false,
            effort: None,
            web_search_requests: 0,
            tokens: counts,
        },
        five_hour_delta: 0.0,
        weekly_delta,
    }
}

fn input<'a>(
    events: &'a [EventAttribution],
    titles: &'a BTreeMap<String, String>,
    pricing: &'a Pricing,
) -> SessionInput<'a> {
    SessionInput {
        events,
        titles,
        pricing,
    }
}

#[test]
fn list_orders_by_weekly_delta_then_cost_and_fills_row_fields() {
    let pricing = Pricing::bundled().expect("bundled pricing");
    let mut titles = BTreeMap::new();
    titles.insert("paid".to_owned(), "重构登录模块".to_owned());
    titles.insert("blank".to_owned(), "   ".to_owned());
    let old = 8 * 24 * 60;
    let events = vec![
        event(
            Tool::Claude,
            "paid",
            "claude-sonnet-5-5",
            Some("/work/token-audit"),
            30,
            tokens(10, 0, 10),
            1.0,
        ),
        event(
            Tool::Claude,
            "paid",
            "claude-sonnet-5-5",
            Some("/work/token-audit"),
            120,
            tokens(10, 0, 10),
            0.5,
        ),
        {
            let mut item = event(
                Tool::Claude,
                "costly",
                "claude-sonnet-5-5",
                Some("/work/token-audit"),
                40,
                tokens(0, 0, 8_000),
                2.0,
            );
            item.event.source = Source::ClaudeDesktop;
            item
        },
        event(
            Tool::Claude,
            "cheap",
            "claude-opus-5-5",
            Some("/work/other"),
            50,
            tokens(0, 0, 10),
            2.0,
        ),
        event(
            Tool::Claude,
            "cheap",
            "claude-sonnet-5-5",
            Some("/work/other"),
            20,
            tokens(0, 0, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "cheap",
            "claude-sonnet-5-5",
            Some("/work/other"),
            18,
            tokens(0, 0, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "span",
            "claude-opus-5-5",
            Some("/old/project"),
            old,
            tokens(0, 0, 50_000),
            50.0,
        ),
        {
            let mut item = event(
                Tool::Claude,
                "span",
                "claude-sonnet-5-5",
                Some("/work/token-audit"),
                15,
                tokens(25, 75, 100),
                0.0,
            );
            item.event.source = Source::ClaudeDesktop;
            item
        },
        {
            let mut item = event(
                Tool::Codex,
                "abcd1234-session",
                "gpt-6-sol",
                Some(r"C:\work\gaobao-api"),
                10,
                tokens(0, 0, 10),
                0.0,
            );
            item.event.source = Source::CodexDesktop;
            item
        },
        event(
            Tool::Codex,
            "blank",
            "gpt-6-sol",
            Some("/tmp/demo"),
            5,
            tokens(0, 0, 10),
            0.0,
        ),
    ];
    let rows = list_sessions(&input(&events, &titles, &pricing), 7, now());
    let ids: Vec<_> = rows.iter().map(|row| row.session_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "costly",
            "cheap",
            "paid",
            "span",
            "abcd1234-session",
            "blank"
        ]
    );

    let paid = rows
        .iter()
        .find(|row| row.session_id == "paid")
        .expect("paid");
    assert_eq!(paid.title.as_deref(), Some("重构登录模块"));
    assert_eq!(paid.display_name, "重构登录模块");
    assert_eq!(paid.project.as_deref(), Some("/work/token-audit"));
    assert_eq!(paid.project_name, "token-audit");
    assert_eq!(paid.source, Source::ClaudeCli);
    assert_eq!(paid.model, "claude-sonnet-5-5");
    assert_eq!(paid.requests, 2);
    assert_eq!(paid.weekly_delta, 1.5);
    assert!(paid.has_quota_data);
    assert_eq!(paid.first_at, at(120));
    assert_eq!(paid.last_at, at(30));

    let cheap = rows
        .iter()
        .find(|row| row.session_id == "cheap")
        .expect("cheap");
    assert_eq!(cheap.model, "claude-sonnet-5-5");
    assert_eq!(cheap.requests, 3);
    let costly = rows
        .iter()
        .find(|row| row.session_id == "costly")
        .expect("costly");
    assert!(costly.estimated_cost_usd > cheap.estimated_cost_usd);

    let span = rows
        .iter()
        .find(|row| row.session_id == "span")
        .expect("span");
    assert_eq!(span.requests, 1);
    assert_eq!(span.model, "claude-sonnet-5-5");
    assert_eq!(span.total_tokens, 200);
    assert_eq!(span.cache_hit_rate, Some(0.75));
    assert_eq!(span.weekly_delta, 0.0);
    assert_eq!(span.source, Source::ClaudeDesktop);
    assert_eq!(span.project_name, "token-audit");
    assert!(span.has_quota_data);
    assert_eq!(span.first_at, at(15));

    let untitled = rows
        .iter()
        .find(|row| row.session_id == "abcd1234-session")
        .expect("untitled");
    assert_eq!(untitled.title, None);
    assert_eq!(untitled.display_name, "gaobao-api · abcd1234");
    assert_eq!(untitled.project.as_deref(), Some(r"C:\work\gaobao-api"));
    assert_eq!(untitled.project_name, "gaobao-api");
    assert_eq!(untitled.tool, Tool::Codex);
    assert!(!untitled.has_quota_data);

    let blank = rows
        .iter()
        .find(|row| row.session_id == "blank")
        .expect("blank");
    assert_eq!(blank.title, None);
    assert_eq!(blank.display_name, "demo · blank");
}

#[test]
fn detail_orders_prompts_before_requests_and_attaches_tags() {
    let pricing = Pricing::bundled().expect("bundled pricing");
    let titles = BTreeMap::from([("sess-1".to_owned(), "重构登录模块".to_owned())]);
    let start = Utc
        .with_ymd_and_hms(2026, 10, 20, 14, 52, 3)
        .single()
        .expect("start");
    let later = start + Duration::minutes(71);
    let after = later + Duration::minutes(1);
    let counts = tokens(10, 20, 30);
    let mut main = event(
        Tool::Claude,
        "sess-1",
        "claude-sonnet-5-5",
        Some("/work/token-audit"),
        0,
        counts,
        4.0,
    );
    main.event.timestamp = start;
    let mut subagent = event(
        Tool::Claude,
        "sess-1",
        "claude-sonnet-5-5",
        Some("/work/token-audit"),
        0,
        counts,
        1.1,
    );
    subagent.event.timestamp = later;
    subagent.event.is_subagent = true;
    let mut third = event(
        Tool::Claude,
        "sess-1",
        "claude-opus-5-5",
        Some("/work/token-audit"),
        0,
        counts,
        3.6,
    );
    third.event.timestamp = after;
    let mut other = event(
        Tool::Codex,
        "sess-1",
        "gpt-6-sol",
        Some("/elsewhere"),
        0,
        counts,
        0.0,
    );
    other.event.timestamp = start;
    let events = vec![third.clone(), main.clone(), subagent.clone(), other];
    let prompts = vec![
        PromptEvent {
            tool: Tool::Codex,
            session_id: "sess-1".to_owned(),
            timestamp: start,
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "sess-1".to_owned(),
            timestamp: later,
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "sess-1".to_owned(),
            timestamp: start,
        },
    ];
    let tags = vec![
        EventTag {
            tool: Tool::Claude,
            session_id: "other".to_owned(),
            request_id: None,
            at: after,
            tag: "不该出现".to_owned(),
            severity: Severity::Error,
        },
        EventTag {
            tool: Tool::Claude,
            session_id: "sess-1".to_owned(),
            request_id: None,
            at: after,
            tag: "缓存已过期".to_owned(),
            severity: Severity::Error,
        },
        EventTag {
            tool: Tool::Claude,
            session_id: "sess-1".to_owned(),
            request_id: None,
            at: later,
            tag: "子代理".to_owned(),
            severity: Severity::Warning,
        },
    ];
    let detail = session_detail(
        &input(&events, &titles, &pricing),
        &prompts,
        &tags,
        Tool::Claude,
        "sess-1",
    )
    .expect("session");
    assert_eq!(detail.display_name, "重构登录模块");
    assert_eq!(detail.project_name, "token-audit");
    assert_eq!(detail.model, "claude-sonnet-5-5");
    assert_eq!(detail.requests, 3);
    assert!(detail.has_quota_data);
    assert!(session_detail(
        &input(&events, &titles, &pricing),
        &prompts,
        &tags,
        Tool::Claude,
        "missing",
    )
    .is_none());

    assert!(matches!(
        &detail.entries[0],
        TimelineEntry::Prompt {
            at,
            gap_minutes_from_previous_prompt: None,
        } if *at == start
    ));
    match &detail.entries[1] {
        TimelineEntry::Request {
            at,
            index,
            is_subagent,
            tag,
            ..
        } => {
            assert_eq!(*at, start);
            assert_eq!(*index, 1);
            assert!(!is_subagent);
            assert!(tag.is_none());
        }
        TimelineEntry::Prompt { .. } => panic!("prompt should not follow itself"),
    }
    assert!(matches!(
        &detail.entries[2],
        TimelineEntry::Prompt {
            at,
            gap_minutes_from_previous_prompt: Some(71),
        } if *at == later
    ));
    match &detail.entries[3] {
        TimelineEntry::Request {
            index,
            is_subagent,
            tag,
            weekly_delta,
            ..
        } => {
            assert_eq!(*index, 2);
            assert!(is_subagent);
            assert_eq!(*weekly_delta, 1.1);
            let tag = tag.as_ref().expect("subagent tag");
            assert_eq!(tag.tag, "子代理");
            assert_eq!(tag.severity, Severity::Warning);
        }
        TimelineEntry::Prompt { .. } => panic!("expected the subagent request"),
    }
    match &detail.entries[4] {
        TimelineEntry::Request {
            index,
            is_subagent,
            tag,
            ..
        } => {
            assert_eq!(*index, 3);
            assert!(!is_subagent);
            let tag = tag.as_ref().expect("rebuild tag");
            assert_eq!(tag.tag, "缓存已过期");
            assert_eq!(tag.severity, Severity::Error);
        }
        TimelineEntry::Prompt { .. } => panic!("expected the third request"),
    }
    assert_eq!(detail.entries.len(), 5);
}

#[test]
fn detail_keeps_same_timestamp_tags_on_the_matching_request() {
    let pricing = Pricing::bundled().expect("bundled pricing");
    let titles = BTreeMap::new();
    let moment = Utc
        .with_ymd_and_hms(2026, 10, 20, 14, 52, 3)
        .single()
        .expect("moment");
    let mut early = event(
        Tool::Claude,
        "s",
        "claude-sonnet-5-5",
        Some("/work/token-audit"),
        0,
        tokens(1, 0, 1),
        1.0,
    );
    early.event.timestamp = moment;
    early.event.request_id = Some("z".to_owned());
    let mut late = event(
        Tool::Claude,
        "s",
        "claude-sonnet-5-5",
        Some("/work/token-audit"),
        0,
        tokens(1, 0, 1),
        2.0,
    );
    late.event.timestamp = moment;
    late.event.request_id = Some("a".to_owned());
    late.event.is_subagent = true;
    let mut plain = event(
        Tool::Claude,
        "s",
        "claude-sonnet-5-5",
        Some("/work/token-audit"),
        0,
        tokens(1, 0, 1),
        0.0,
    );
    plain.event.timestamp = moment;
    plain.event.request_id = Some("m".to_owned());
    // Tag order is deliberately not request order. The untagged middle request
    // must not absorb a neighbor's tag.
    let tags = vec![
        EventTag {
            tool: Tool::Claude,
            session_id: "s".to_owned(),
            request_id: Some("z".to_owned()),
            at: moment,
            tag: "新会话启动开销".to_owned(),
            severity: Severity::Warning,
        },
        EventTag {
            tool: Tool::Claude,
            session_id: "s".to_owned(),
            request_id: Some("a".to_owned()),
            at: moment,
            tag: "子代理".to_owned(),
            severity: Severity::Warning,
        },
    ];
    let detail = session_detail(
        &input(&[early, late, plain], &titles, &pricing),
        &[],
        &tags,
        Tool::Claude,
        "s",
    )
    .expect("session");
    match &detail.entries[0] {
        TimelineEntry::Request {
            index,
            is_subagent,
            tag,
            ..
        } => {
            assert_eq!(*index, 1);
            assert!(is_subagent);
            assert_eq!(tag.as_ref().expect("tag").tag, "子代理");
        }
        TimelineEntry::Prompt { .. } => panic!("expected the earlier request id"),
    }
    match &detail.entries[1] {
        TimelineEntry::Request {
            index,
            is_subagent,
            tag,
            ..
        } => {
            assert_eq!(*index, 2);
            assert!(!is_subagent);
            assert!(tag.is_none());
        }
        TimelineEntry::Prompt { .. } => panic!("expected the untagged request"),
    }
    match &detail.entries[2] {
        TimelineEntry::Request {
            index,
            is_subagent,
            tag,
            ..
        } => {
            assert_eq!(*index, 3);
            assert!(!is_subagent);
            assert_eq!(tag.as_ref().expect("tag").tag, "新会话启动开销");
        }
        TimelineEntry::Prompt { .. } => panic!("expected the later request id"),
    }
}
