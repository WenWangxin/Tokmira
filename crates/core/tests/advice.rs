use chrono::{DateTime, Duration, TimeZone, Utc};
use token_audit_core::advice::{
    build_advice, percentile, AdviceConfig, AdviceInput, AdviceReport, Severity,
};
use token_audit_core::attribution::EventAttribution;
use token_audit_core::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0)
        .single()
        .expect("fixed time")
}

fn at(minutes_ago: i64) -> DateTime<Utc> {
    now() - Duration::minutes(minutes_ago)
}

fn event(
    tool: Tool,
    session: &str,
    minutes_ago: i64,
    model: &str,
    tokens: TokenCounts,
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
            project: Some("/fake/project".to_owned()),
            is_subagent: false,
            tokens,
        },
        five_hour_delta: 0.0,
        weekly_delta,
    }
}

fn claude_tokens(cache_read: u64, write_1h: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input: 10,
        output,
        cache_read,
        cache_write_1h: write_1h,
        ..TokenCounts::default()
    }
}

fn report(
    events: &[EventAttribution],
    prompts: &[PromptEvent],
    dismissed: &[String],
) -> AdviceReport {
    let pricing = Pricing::bundled().expect("bundled pricing");
    let config = AdviceConfig::bundled().expect("bundled advice config");
    build_advice(
        &AdviceInput {
            events,
            prompts,
            pricing: &pricing,
            config: &config,
            dismissed,
        },
        7,
        now(),
    )
}

fn ids(report: &AdviceReport, tool: Tool) -> Vec<String> {
    report
        .tools
        .iter()
        .find(|t| t.tool == tool)
        .map(|t| t.items.iter().map(|i| i.id.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn cache_rebuild_needs_gap_beyond_ttl_and_cold_cache() {
    // One-hour cache writes: a 90 minute gap with a cold cache is a rebuild.
    let events = vec![
        event(
            Tool::Claude,
            "s1",
            200,
            "claude-opus-5-5",
            claude_tokens(0, 30_000, 100),
            0.0,
        ),
        event(
            Tool::Claude,
            "s1",
            110,
            "claude-opus-5-5",
            claude_tokens(1_000, 50_000, 100),
            0.0,
        ),
        // 30 minutes later: within the one-hour TTL, not a rebuild even though the cache is cold.
        event(
            Tool::Claude,
            "s1",
            80,
            "claude-opus-5-5",
            claude_tokens(0, 50_000, 100),
            0.0,
        ),
    ];
    let report = report(&events, &[], &[]);
    let claude = &report.tools[0];
    let rebuild = claude
        .items
        .iter()
        .find(|i| i.id == "cache_rebuild")
        .expect("rebuild detected");
    assert_eq!(rebuild.occurrences, 1);
    assert_eq!(rebuild.overhead_tokens, 50_000);
    // (1h write $8 − cache read $0.20) per million × 50k tokens.
    assert!((rebuild.avoidable_cost_usd - 50_000.0 * 7.8 / 1e6).abs() < 1e-9);
    assert!(rebuild.facts.iter().any(|f| f == "间隔约 90 分钟"));
}

#[test]
fn codex_rebuild_uses_uncached_input_and_thirty_minute_gap() {
    let tokens = |input, cached| TokenCounts {
        input,
        cache_read: cached,
        output: 50,
        ..TokenCounts::default()
    };
    let events = vec![
        event(Tool::Codex, "c1", 100, "gpt-6-sol", tokens(30_000, 0), 0.0),
        event(
            Tool::Codex,
            "c1",
            60,
            "gpt-6-sol",
            tokens(40_000, 1_000),
            0.0,
        ),
        event(
            Tool::Codex,
            "c1",
            40,
            "gpt-6-sol",
            tokens(40_000, 1_000),
            0.0,
        ),
    ];
    let report = report(&events, &[], &[]);
    let rebuild = report.tools[0]
        .items
        .iter()
        .find(|i| i.id == "cache_rebuild")
        .expect("codex rebuild");
    assert_eq!(rebuild.occurrences, 1);
    assert_eq!(rebuild.overhead_tokens, 40_000);
}

#[test]
fn session_startup_counts_only_sessions_that_start_in_the_period() {
    let old_start = 8 * 24 * 60;
    let events = vec![
        event(
            Tool::Claude,
            "old",
            old_start,
            "claude-opus-5-5",
            claude_tokens(0, 40_000, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "old",
            60,
            "claude-opus-5-5",
            claude_tokens(40_000, 100, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "new",
            30,
            "claude-opus-5-5",
            claude_tokens(0, 60_000, 10),
            0.0,
        ),
    ];
    let report = report(&events, &[], &[]);
    let startup = report.tools[0]
        .items
        .iter()
        .find(|i| i.id == "session_startup")
        .expect("startup");
    assert_eq!(startup.occurrences, 1);
    assert_eq!(startup.overhead_tokens, 60_000);
    assert!(!startup.avoidable);
    assert_eq!(startup.severity, Severity::Warning);
}

#[test]
fn long_context_threshold_and_excess() {
    let events = vec![
        event(
            Tool::Claude,
            "s",
            50,
            "claude-opus-5-5",
            claude_tokens(250_000, 0, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "s",
            40,
            "claude-opus-5-5",
            claude_tokens(400_000, 0, 10),
            0.0,
        ),
    ];
    let report = report(&events, &[], &[]);
    let long = report.tools[0]
        .items
        .iter()
        .find(|i| i.id == "long_context")
        .expect("long context");
    assert_eq!(long.occurrences, 1);
    // Context 400,010 − target 150,000, capped by cache read 400,000.
    assert_eq!(long.overhead_tokens, 250_010);
}

#[test]
fn light_task_is_judged_per_prompt() {
    let prompts = vec![
        PromptEvent {
            tool: Tool::Claude,
            session_id: "s".into(),
            timestamp: at(100),
        },
        PromptEvent {
            tool: Tool::Claude,
            session_id: "s".into(),
            timestamp: at(50),
        },
    ];
    let mut events = vec![
        // Prompt 1: one short Opus request -> light task.
        event(
            Tool::Claude,
            "s",
            99,
            "claude-opus-5-5",
            claude_tokens(40_000, 0, 200),
            0.0,
        ),
    ];
    // Prompt 2: five short Opus requests (an agent loop) -> not a light task.
    for minute in 45..50 {
        events.push(event(
            Tool::Claude,
            "s",
            minute,
            "claude-opus-5-5",
            claude_tokens(40_000, 0, 100),
            0.0,
        ));
    }
    let report = report(&events, &prompts, &[]);
    let light = report.tools[0]
        .items
        .iter()
        .find(|i| i.id == "expensive_light_task")
        .expect("light task");
    assert_eq!(light.occurrences, 1);
    assert_eq!(light.title, "轻量任务使用了 Opus");
    assert!(light.avoidable_cost_usd > 0.0);
}

#[test]
fn many_subagents_needs_share_and_count() {
    let mut events = vec![event(
        Tool::Claude,
        "s",
        100,
        "claude-opus-5-5",
        claude_tokens(1_000, 0, 10),
        0.0,
    )];
    for minute in 0..12 {
        let mut sub = event(
            Tool::Claude,
            "s",
            90 - minute,
            "claude-opus-5-5",
            claude_tokens(60_000, 0, 50),
            0.0,
        );
        sub.event.is_subagent = true;
        events.push(sub);
    }
    let report1 = report(&events, &[], &[]);
    assert!(ids(&report1, Tool::Claude).contains(&"many_subagents".to_owned()));
    events.truncate(6);
    let report2 = report(&events, &[], &[]);
    assert!(!ids(&report2, Tool::Claude).contains(&"many_subagents".to_owned()));
}

#[test]
fn weekly_percent_follows_attributed_delta_or_is_none() {
    let rebuild_events = |delta| {
        vec![
            event(
                Tool::Claude,
                "s",
                200,
                "claude-opus-5-5",
                claude_tokens(0, 30_000, 100),
                0.0,
            ),
            event(
                Tool::Claude,
                "s",
                100,
                "claude-opus-5-5",
                claude_tokens(0, 50_000, 0),
                delta,
            ),
        ]
    };
    let with_quota = report(&rebuild_events(2.0), &[], &[]);
    let item = with_quota.tools[0]
        .items
        .iter()
        .find(|i| i.id == "cache_rebuild")
        .expect("rebuild");
    let pricing = Pricing::bundled().expect("pricing");
    let cost = pricing
        .cost_usd("claude-opus-5-5", &claude_tokens(0, 50_000, 0))
        .expect("priced");
    let expected = 2.0 * (item.avoidable_cost_usd / cost);
    assert!((item.avoidable_weekly_pct.expect("has quota") - expected).abs() < 1e-9);
    assert!(with_quota.tools[0].has_quota_data);

    let without = report(&rebuild_events(0.0), &[], &[]);
    let item = without.tools[0]
        .items
        .iter()
        .find(|i| i.id == "cache_rebuild")
        .expect("rebuild");
    assert!(item.avoidable_weekly_pct.is_none());
    assert!(without.tools[0].summary.avoidable_weekly_pct.is_none());
}

#[test]
fn dismissed_items_are_hidden_and_counted() {
    let events = vec![
        event(
            Tool::Claude,
            "s",
            200,
            "claude-opus-5-5",
            claude_tokens(0, 30_000, 100),
            0.0,
        ),
        event(
            Tool::Claude,
            "s",
            100,
            "claude-opus-5-5",
            claude_tokens(0, 50_000, 100),
            0.0,
        ),
    ];
    let report = report(&events, &[], &["claude:cache_rebuild".to_owned()]);
    let claude = &report.tools[0];
    assert!(!claude.items.iter().any(|i| i.id == "cache_rebuild"));
    assert_eq!(claude.hidden, 1);
}

#[test]
fn prompt_tags_attach_to_the_prompt_of_tagged_events() {
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "s".into(),
        timestamp: at(101),
    }];
    let events = vec![
        event(
            Tool::Claude,
            "s",
            200,
            "claude-opus-5-5",
            claude_tokens(0, 30_000, 900),
            0.0,
        ),
        event(
            Tool::Claude,
            "s",
            100,
            "claude-opus-5-5",
            claude_tokens(0, 50_000, 900),
            0.0,
        ),
    ];
    let report = report(&events, &prompts, &[]);
    let tag = report
        .prompt_tags
        .iter()
        .find(|t| t.prompt_at == at(101))
        .expect("tag for the prompt");
    assert_eq!(tag.tag, "缓存已过期");
}

#[test]
fn percentile_interpolates() {
    assert_eq!(percentile(&[], 0.5), None);
    assert_eq!(percentile(&[10.0], 0.25), Some(10.0));
    assert_eq!(percentile(&[0.0, 10.0, 20.0, 30.0], 0.25), Some(7.5));
}

#[test]
fn config_override_replaces_values_and_keeps_others() {
    let config = AdviceConfig::bundled()
        .expect("bundled")
        .with_override("[long_context]\nthreshold_tokens = 100\ntarget_tokens = 50\n[links.cache_rebuild]\nlabel = \"看看\"\nurl = \"https://example.invalid/guide\"\n")
        .expect("override");
    assert_eq!(config.long_context.threshold_tokens, 100);
    assert_eq!(config.cache_rebuild.min_rebuild_tokens, 20_000);
    assert_eq!(
        config.links.get("cache_rebuild").map(|l| l.url.as_str()),
        Some("https://example.invalid/guide")
    );
    assert!(AdviceConfig::bundled()
        .expect("bundled")
        .with_override("not = [valid")
        .is_err());
}

#[test]
fn small_overhead_does_not_tag_an_expensive_prompt() {
    let prompts = vec![PromptEvent {
        tool: Tool::Claude,
        session_id: "s".into(),
        timestamp: at(101),
    }];
    // Start-up write of 20k tokens, then a long and expensive piece of work in the same prompt.
    let mut events = vec![event(
        Tool::Claude,
        "s",
        100,
        "claude-opus-5-5",
        claude_tokens(0, 20_000, 50),
        0.0,
    )];
    for minute in 60..99 {
        events.push(event(
            Tool::Claude,
            "s",
            minute,
            "claude-opus-5-5",
            claude_tokens(200_000, 0, 2_000),
            0.0,
        ));
    }
    let report = report(&events, &prompts, &[]);
    assert!(report.prompt_tags.iter().all(|t| t.tag != "新会话启动开销"));
    // Request tags are not gated by the prompt's 30% cost share.
    assert_eq!(report.event_tags.len(), 1);
    assert_eq!(report.event_tags[0].tag, "新会话启动开销");
    assert_eq!(report.event_tags[0].at, at(100));
    assert_eq!(report.event_tags[0].severity, Severity::Warning);
}

#[test]
fn event_tags_keep_one_per_request_preferring_error_then_earlier_rule() {
    let heavy = |session: &str, delta: f64| {
        event(
            Tool::Claude,
            session,
            30,
            "claude-opus-5-5",
            TokenCounts {
                input: 10,
                output: 100,
                cache_read: 400_000,
                cache_write_1h: 40_000,
                ..TokenCounts::default()
            },
            delta,
        )
    };
    // Start-up (earlier, Warning) and long context (later) hit the same request.
    let warning = report(&[heavy("s", 0.0)], &[], &[]);
    assert_eq!(warning.event_tags.len(), 1);
    assert_eq!(warning.event_tags[0].tag, "新会话启动开销");
    assert_eq!(warning.event_tags[0].severity, Severity::Warning);

    let error = report(&[heavy("s", 100.0)], &[], &[]);
    assert_eq!(error.event_tags.len(), 1);
    assert_eq!(error.event_tags[0].tag, "上下文过长");
    assert_eq!(error.event_tags[0].severity, Severity::Error);
    assert_eq!(error.event_tags[0].at, at(30));
}

#[test]
fn event_tags_follow_request_order_when_timestamps_match() {
    let moment = at(30);
    let mut startup = event(
        Tool::Claude,
        "s",
        30,
        "claude-opus-5-5",
        TokenCounts {
            input: 10,
            output: 20,
            cache_write_1h: 30_000,
            ..TokenCounts::default()
        },
        0.0,
    );
    startup.event.timestamp = moment;
    startup.event.request_id = Some("a".to_owned());
    let mut long = event(
        Tool::Claude,
        "s",
        30,
        "claude-opus-5-5",
        TokenCounts {
            input: 10,
            output: 20,
            cache_read: 400_000,
            ..TokenCounts::default()
        },
        0.0,
    );
    long.event.timestamp = moment;
    long.event.request_id = Some("z".to_owned());
    let report = report(&[startup, long], &[], &[]);
    let tags: Vec<_> = report
        .event_tags
        .iter()
        .map(|tag| (tag.tag.as_str(), tag.request_id.as_deref()))
        .collect();
    assert_eq!(
        tags,
        vec![("新会话启动开销", Some("a")), ("上下文过长", Some("z"))]
    );
}
