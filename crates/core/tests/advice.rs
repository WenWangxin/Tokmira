use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Duration, TimeZone, Utc};
use token_audit_core::advice::{
    build_advice, percentile, quota_advice_states, AdviceConfig, AdviceInput, AdviceReport,
    QuotaAdviceState, Severity,
};
use token_audit_core::attribution::EventAttribution;
use token_audit_core::environment::EnvironmentSnapshot;
use token_audit_core::model::{CompactEvent, PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{
    normalize_claude_plan, ClaudeSnapshot, CodexSnapshot, StoredWindow,
};

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
            fast: false,
            effort: None,
            web_search_requests: 0,
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
            compactions: &[],
            environment: None,
            quotas: &[],
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

fn m12_report(
    events: &[EventAttribution],
    prompts: &[PromptEvent],
    compactions: &[CompactEvent],
    quotas: &[QuotaAdviceState],
    environment: Option<&EnvironmentSnapshot>,
) -> AdviceReport {
    let pricing = Pricing::bundled().expect("prices");
    let config = AdviceConfig::bundled().expect("config");
    build_advice(
        &AdviceInput {
            events,
            prompts,
            pricing: &pricing,
            config: &config,
            dismissed: &[],
            compactions,
            environment,
            quotas,
        },
        7,
        now(),
    )
}

fn prompt(tool: Tool, session: &str, minutes_ago: i64) -> PromptEvent {
    PromptEvent {
        tool,
        session_id: session.to_owned(),
        timestamp: at(minutes_ago),
    }
}

fn compact(tool: Tool, session: &str, minutes_ago: i64, trigger: &str) -> CompactEvent {
    CompactEvent {
        tool,
        session_id: session.to_owned(),
        timestamp: at(minutes_ago),
        trigger: Some(trigger.to_owned()),
        pre_tokens: Some(90_000),
        post_tokens: Some(10_000),
    }
}

#[test]
fn m12_usage_rules_have_hit_and_miss_boundaries() {
    let base =
        |tool, session, minute, model, tokens| event(tool, session, minute, model, tokens, 0.0);
    let first = base(
        Tool::Claude,
        "switch",
        2,
        "claude-opus-5-5",
        claude_tokens(0, 30_000, 100),
    );
    let second = base(
        Tool::Claude,
        "switch",
        1,
        "claude-sonnet-5-5",
        claude_tokens(100, 30_000, 100),
    );
    assert!(ids(
        &m12_report(&[first.clone(), second.clone()], &[], &[], &[], None),
        Tool::Claude
    )
    .contains(&"model_switch".into()));
    let mut ignored = second.clone();
    ignored.event.model = "claude-haiku-5-5".into();
    assert!(!ids(
        &m12_report(&[first, ignored], &[], &[], &[], None),
        Tool::Claude
    )
    .contains(&"model_switch".into()));

    let start = base(
        Tool::Claude,
        "spike",
        3,
        "claude-opus-5-5",
        claude_tokens(0, 40_000, 100),
    );
    let spike = base(
        Tool::Claude,
        "spike",
        2,
        "claude-opus-5-5",
        claude_tokens(200_000, 50_000, 100),
    );
    let tail = base(
        Tool::Claude,
        "spike",
        1,
        "claude-opus-5-5",
        claude_tokens(250_000, 0, 100),
    );
    assert!(ids(
        &m12_report(
            &[start.clone(), spike.clone(), tail.clone()],
            &[],
            &[],
            &[],
            None
        ),
        Tool::Claude
    )
    .contains(&"context_spike".into()));
    let boundary = CompactEvent {
        tool: Tool::Claude,
        session_id: "spike".into(),
        timestamp: at(3) + Duration::seconds(30),
        trigger: Some("manual".into()),
        pre_tokens: Some(90_000),
        post_tokens: Some(10_000),
    };
    assert!(!ids(
        &m12_report(
            &[start.clone(), spike.clone(), tail.clone()],
            &[],
            &[boundary],
            &[],
            None
        ),
        Tool::Claude
    )
    .contains(&"context_spike".into()));
    let mut small = spike;
    small.event.tokens.cache_write_1h = 19_999;
    assert!(!ids(
        &m12_report(&[start, small, tail], &[], &[], &[], None),
        Tool::Claude
    )
    .contains(&"context_spike".into()));

    let requests: Vec<_> = (10..50)
        .map(|minute| {
            base(
                Tool::Claude,
                "runaway",
                minute,
                "claude-sonnet-5-5",
                claude_tokens(10, 0, 100),
            )
        })
        .collect();
    let prompts = [prompt(Tool::Claude, "runaway", 50)];
    assert!(ids(
        &m12_report(&requests, &prompts, &[], &[], None),
        Tool::Claude
    )
    .contains(&"runaway_prompt".into()));
    assert!(!ids(
        &m12_report(&requests[..39], &prompts, &[], &[], None),
        Tool::Claude
    )
    .contains(&"runaway_prompt".into()));
    let mut with_subagent = requests[..39].to_vec();
    let mut extra = requests[39].clone();
    extra.event.is_subagent = true;
    with_subagent.push(extra);
    assert!(ids(
        &m12_report(&with_subagent, &prompts, &[], &[], None),
        Tool::Claude
    )
    .contains(&"runaway_prompt".into()));

    let subagents: Vec<_> = (1..=5)
        .map(|minute| {
            let mut item = base(
                Tool::Claude,
                "sub",
                minute,
                "claude-opus-5-5",
                claude_tokens(0, 1_000, 100),
            );
            item.event.is_subagent = true;
            item
        })
        .collect();
    assert!(
        ids(&m12_report(&subagents, &[], &[], &[], None), Tool::Claude)
            .contains(&"expensive_subagent".into())
    );
    assert!(!ids(
        &m12_report(&subagents[..4], &[], &[], &[], None),
        Tool::Claude
    )
    .contains(&"expensive_subagent".into()));

    let reasoning = base(
        Tool::Codex,
        "reason",
        1,
        "gpt-6-astra",
        TokenCounts {
            input: 100,
            output: 3_100,
            reasoning: 3_000,
            ..TokenCounts::default()
        },
    );
    let prompts = [prompt(Tool::Codex, "reason", 2)];
    assert!(ids(
        &m12_report(std::slice::from_ref(&reasoning), &prompts, &[], &[], None),
        Tool::Codex
    )
    .contains(&"heavy_reasoning".into()));
    let mut light = reasoning;
    light.event.tokens.reasoning = 1_000;
    assert!(
        !ids(&m12_report(&[light], &prompts, &[], &[], None), Tool::Codex)
            .contains(&"heavy_reasoning".into())
    );

    let mut fast = base(
        Tool::Claude,
        "fast",
        1,
        "claude-opus-5-5",
        claude_tokens(0, 1_000, 100),
    );
    fast.event.fast = true;
    let report = m12_report(std::slice::from_ref(&fast), &[], &[], &[], None);
    let item = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "fast_mode")
        .expect("fast mode");
    assert!(item.avoidable);
    assert!(item.overhead_cost_usd > 0.0);
    let mut dated = fast.clone();
    dated.event.model = "claude-opus-5-5-20261027".into();
    let dated_report = m12_report(&[dated], &[], &[], &[], None);
    assert!(dated_report.tools[0]
        .items
        .iter()
        .any(|item| item.id == "fast_mode" && item.avoidable));
    let mut unpriced_model = fast.clone();
    unpriced_model.event.model = "claude-sonnet-5-5".into();
    let unpriced_report = m12_report(&[unpriced_model], &[], &[], &[], None);
    assert!(unpriced_report.tools[0]
        .items
        .iter()
        .any(|item| item.id == "fast_mode" && !item.avoidable));
    let pricing = Pricing::bundled().expect("prices");
    let mut config = AdviceConfig::bundled().expect("config");
    config.fast_mode.claude_price_multiplier = 0.0;
    let fast_events = [fast.clone()];
    let priced = build_advice(
        &AdviceInput {
            events: &fast_events,
            prompts: &[],
            pricing: &pricing,
            config: &config,
            dismissed: &[],
            compactions: &[],
            environment: None,
            quotas: &[],
        },
        7,
        now(),
    );
    let premium = priced.tools[0]
        .items
        .iter()
        .find(|item| item.id == "fast_mode")
        .expect("premium");
    assert!(!premium.avoidable);
    assert!(
        (item.avoidable_cost_usd
            - pricing
                .cost_usd(&fast.event.model, &fast.event.tokens)
                .expect("model"))
        .abs()
            < 0.000_000_1
    );
    fast.event.fast = false;
    assert!(
        !ids(&m12_report(&[fast], &[], &[], &[], None), Tool::Claude).contains(&"fast_mode".into())
    );

    let mut effort = base(
        Tool::Claude,
        "effort",
        1,
        "claude-sonnet-5-5",
        claude_tokens(0, 1_000, 100),
    );
    effort.event.effort = Some("xhigh".into());
    let prompts = [prompt(Tool::Claude, "effort", 2)];
    assert!(ids(
        &m12_report(std::slice::from_ref(&effort), &prompts, &[], &[], None),
        Tool::Claude
    )
    .contains(&"high_effort_light_task".into()));
    effort.event.effort = Some("high".into());
    assert!(!ids(
        &m12_report(&[effort], &prompts, &[], &[], None),
        Tool::Claude
    )
    .contains(&"high_effort_light_task".into()));

    let base_compact = base(
        Tool::Claude,
        "compact",
        4,
        "claude-sonnet-5-5",
        claude_tokens(0, 40_000, 100),
    );
    let compactions = [
        compact(Tool::Claude, "compact", 3, "auto"),
        compact(Tool::Claude, "compact", 2, "manual"),
    ];
    assert!(ids(
        &m12_report(
            std::slice::from_ref(&base_compact),
            &[],
            &compactions,
            &[],
            None
        ),
        Tool::Claude
    )
    .contains(&"frequent_compaction".into()));
    assert!(!ids(
        &m12_report(&[base_compact], &[], &compactions[..1], &[], None),
        Tool::Claude
    )
    .contains(&"frequent_compaction".into()));

    let mut web = base(
        Tool::Claude,
        "web",
        1,
        "claude-sonnet-5-5",
        claude_tokens(0, 1_000, 100),
    );
    web.event.web_search_requests = 20;
    assert!(ids(
        &m12_report(std::slice::from_ref(&web), &[], &[], &[], None),
        Tool::Claude
    )
    .contains(&"web_search".into()));
    let mut search_pricing = Pricing::bundled().expect("prices");
    search_pricing.server_tools.web_search_per_1k = 10.0;
    let config = AdviceConfig::bundled().expect("config");
    let web_events = [web.clone()];
    let search = build_advice(
        &AdviceInput {
            events: &web_events,
            prompts: &[],
            pricing: &search_pricing,
            config: &config,
            dismissed: &[],
            compactions: &[],
            environment: None,
            quotas: &[],
        },
        7,
        now(),
    );
    let search_item = search.tools[0]
        .items
        .iter()
        .find(|item| item.id == "web_search")
        .expect("search");
    assert!((search_item.overhead_cost_usd - 0.2).abs() < 0.000_000_1);
    web.event.web_search_requests = 19;
    assert!(
        !ids(&m12_report(&[web], &[], &[], &[], None), Tool::Claude).contains(&"web_search".into())
    );
}

#[test]
fn quota_adapter_uses_only_completed_stored_weeks_for_plan_fit() {
    let mut snapshots = Vec::new();
    for (weeks_ago, percent) in [(4, 10.0), (3, 20.0), (2, 30.0), (1, 40.0)] {
        let reset = now() - Duration::weeks(weeks_ago);
        snapshots.push(ClaudeSnapshot {
            at: reset - Duration::hours(1),
            subscription_type: Some("max_5x".into()),
            rate_limit_tier: None,
            five_hour: None,
            seven_day: Some(StoredWindow {
                used_percent: percent,
                resets_at: Some(reset),
                window_minutes: Some(10_080),
            }),
            weekly_breakdown: None,
        });
    }
    snapshots.push(ClaudeSnapshot {
        at: now() - Duration::hours(1),
        subscription_type: Some("max_5x".into()),
        rate_limit_tier: None,
        five_hour: None,
        seven_day: Some(StoredWindow {
            used_percent: 99.0,
            resets_at: Some(now() + Duration::days(6)),
            window_minutes: Some(10_080),
        }),
        weekly_breakdown: None,
    });
    let states = quota_advice_states(&[], &snapshots, &[], &[], now(), 24);
    let claude = states
        .iter()
        .find(|state| state.tool == Some(Tool::Claude))
        .expect("claude");
    assert_eq!(claude.completed_weekly_max, [40.0, 30.0, 20.0, 10.0]);
    assert_eq!(claude.plan.as_deref(), Some("max_5x"));
    snapshots[0].at -= Duration::days(2);
    let states = quota_advice_states(&[], &snapshots, &[], &[], now(), 24);
    let claude = states
        .iter()
        .find(|state| state.tool == Some(Tool::Claude))
        .expect("claude");
    assert_eq!(claude.completed_weekly_max, [40.0, 30.0, 20.0]);
    let usage = event(
        Tool::Claude,
        "coverage",
        1,
        "claude-sonnet-5-5",
        claude_tokens(100, 0, 10),
        0.0,
    );
    assert!(
        !ids(&m12_report(&[usage], &[], &[], &states, None), Tool::Claude)
            .contains(&"plan_fit".into())
    );
    let codex = vec![CodexSnapshot {
        at: now() - Duration::weeks(1) - Duration::hours(25),
        limit_id: Some("codex".into()),
        plan_type: Some("plus".into()),
        primary: None,
        secondary: Some(StoredWindow {
            used_percent: 60.0,
            resets_at: Some(now() - Duration::weeks(1)),
            window_minutes: Some(10_080),
        }),
        reset_credits_available: None,
    }];
    let states = quota_advice_states(&[], &[], &codex, &[], now(), 24);
    let codex = states
        .iter()
        .find(|state| state.tool == Some(Tool::Codex))
        .expect("codex");
    assert!(codex.completed_weekly_max.is_empty());
}

#[test]
fn max_tier_enables_plan_fit_but_unknown_tier_does_not() {
    assert_eq!(
        normalize_claude_plan(Some("max"), Some("default_claude_max_5x")),
        Some("max_5x".into())
    );
    assert_eq!(
        normalize_claude_plan(Some("max"), Some("default_claude_max_20x")),
        Some("max_20x".into())
    );
    assert_eq!(normalize_claude_plan(Some("max"), None), None);

    let mut snapshots = (1..=4)
        .map(|weeks_ago| {
            let reset = now() - Duration::weeks(weeks_ago);
            ClaudeSnapshot {
                at: reset - Duration::hours(1),
                subscription_type: Some("max".into()),
                rate_limit_tier: Some("default_claude_max_5x".into()),
                five_hour: None,
                seven_day: Some(StoredWindow {
                    used_percent: 10.0,
                    resets_at: Some(reset),
                    window_minutes: Some(10_080),
                }),
                weekly_breakdown: None,
            }
        })
        .collect::<Vec<_>>();
    snapshots.push(ClaudeSnapshot {
        at: now() - Duration::hours(1),
        subscription_type: Some("max".into()),
        rate_limit_tier: Some("default_claude_max_5x".into()),
        five_hour: None,
        seven_day: None,
        weekly_breakdown: None,
    });
    let usage = event(
        Tool::Claude,
        "max-plan",
        1,
        "claude-opus-5-5",
        claude_tokens(0, 100, 10),
        0.0,
    );
    let states = quota_advice_states(&[], &snapshots, &[], &[], now(), 24);
    assert_eq!(states[0].plan.as_deref(), Some("max_5x"));
    let report = m12_report(std::slice::from_ref(&usage), &[], &[], &states, None);
    assert!(ids(&report, Tool::Claude).contains(&"plan_fit".into()));

    snapshots.last_mut().expect("latest").rate_limit_tier = None;
    let states = quota_advice_states(&[], &snapshots, &[], &[], now(), 24);
    assert_eq!(states[0].plan, None);
    let report = m12_report(&[usage], &[], &[], &states, None);
    assert!(!ids(&report, Tool::Claude).contains(&"plan_fit".into()));
}

#[test]
fn zero_price_m12_rules_tag_requests_but_respect_prompt_cost_share() {
    let mut fast = event(
        Tool::Claude,
        "fast-tag",
        1,
        "claude-sonnet-5-5",
        claude_tokens(100, 0, 10),
        0.0,
    );
    fast.event.fast = true;
    let mut search = event(
        Tool::Claude,
        "search-tag",
        1,
        "claude-sonnet-5-5",
        claude_tokens(100, 0, 10),
        0.0,
    );
    search.event.web_search_requests = 20;
    let events = [
        event(
            Tool::Claude,
            "fast-tag",
            3,
            "claude-sonnet-5-5",
            claude_tokens(0, 1_000, 10),
            0.0,
        ),
        event(
            Tool::Claude,
            "search-tag",
            3,
            "claude-sonnet-5-5",
            claude_tokens(0, 1_000, 10),
            0.0,
        ),
        fast,
        search,
    ];
    let prompts = [
        prompt(Tool::Claude, "fast-tag", 2),
        prompt(Tool::Claude, "search-tag", 2),
    ];
    let mut pricing = Pricing::bundled().expect("prices");
    pricing.server_tools.web_search_per_1k = 0.0;
    let mut config = AdviceConfig::bundled().expect("config");
    config.fast_mode.claude_price_multiplier = 0.0;
    let report = build_advice(
        &AdviceInput {
            events: &events,
            prompts: &prompts,
            pricing: &pricing,
            config: &config,
            dismissed: &[],
            compactions: &[],
            environment: None,
            quotas: &[],
        },
        7,
        now(),
    );
    assert!(report
        .event_tags
        .iter()
        .any(|tag| tag.session_id == "fast-tag" && tag.tag == "快速模式"));
    assert!(report
        .event_tags
        .iter()
        .any(|tag| tag.session_id == "search-tag" && tag.tag == "联网搜索较多"));
    assert!(!report
        .prompt_tags
        .iter()
        .any(|tag| tag.session_id == "fast-tag" && tag.tag == "快速模式"));
    assert!(!report
        .prompt_tags
        .iter()
        .any(|tag| tag.session_id == "search-tag" && tag.tag == "联网搜索较多"));
}

#[test]
fn fast_mode_weekly_share_uses_the_actual_request_cost() {
    let mut fast = event(
        Tool::Claude,
        "fast-quota",
        1,
        "claude-opus-5-5",
        claude_tokens(0, 1_000, 100),
        2.0,
    );
    fast.event.fast = true;
    let report = m12_report(&[fast], &[], &[], &[], None);
    let item = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "fast_mode")
        .expect("fast mode");
    assert_eq!(item.avoidable_weekly_pct, Some(1.0));
    assert_eq!(item.consumed_weekly_pct, Some(1.0));
}

#[test]
fn rule_without_weekly_attribution_shows_cost_even_when_tool_has_quota() {
    let events = [
        event(
            Tool::Claude,
            "switch",
            3,
            "claude-opus-5-5",
            claude_tokens(0, 30_000, 100),
            0.0,
        ),
        event(
            Tool::Claude,
            "switch",
            2,
            "claude-sonnet-5-5",
            claude_tokens(100, 30_000, 100),
            0.0,
        ),
        event(
            Tool::Claude,
            "other",
            1,
            "claude-sonnet-5-5",
            claude_tokens(100, 0, 100),
            2.0,
        ),
    ];
    let report = m12_report(&events, &[], &[], &[], None);
    let tool = report
        .tools
        .iter()
        .find(|tool| tool.tool == Tool::Claude)
        .expect("tool");
    let item = tool
        .items
        .iter()
        .find(|item| item.id == "model_switch")
        .expect("switch");
    assert!(tool.has_quota_data);
    assert!(item.avoidable_cost_usd > 0.0);
    assert_eq!(item.consumed_weekly_pct, None);
    assert_eq!(item.avoidable_weekly_pct, None);
}

#[test]
fn runaway_prompt_facts_describe_only_matching_prompts() {
    let mut events = (160..200)
        .map(|minute| {
            event(
                Tool::Claude,
                "long",
                minute,
                "claude-sonnet-5-5",
                claude_tokens(100, 0, 10),
                0.0,
            )
        })
        .collect::<Vec<_>>();
    events.push(event(
        Tool::Claude,
        "short",
        99,
        "claude-sonnet-5-5",
        claude_tokens(100, 0, 10),
        0.0,
    ));
    let prompts = [
        prompt(Tool::Claude, "long", 200),
        prompt(Tool::Claude, "short", 100),
    ];
    let report = m12_report(&events, &prompts, &[], &[], None);
    let item = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "runaway_prompt")
        .expect("runaway");
    assert!(item
        .facts
        .iter()
        .any(|fact| fact == "平均每次提示 40.0 次请求"));
    assert!(item.facts.iter().any(|fact| fact == "最多一次 40 次"));
}

#[test]
fn m12_quota_rules_and_startup_inventory_have_hit_and_miss_boundaries() {
    let usage = event(
        Tool::Claude,
        "quota",
        1,
        "claude-sonnet-5-5",
        claude_tokens(0, 30_000, 100),
        0.0,
    );
    let busy = QuotaAdviceState {
        tool: Some(Tool::Claude),
        weekly_pct: Some(74.0),
        resets_at: Some(now() + Duration::days(1)),
        predicted_hit_at: Some(now() + Duration::hours(1)),
        used_recently: true,
        ..QuotaAdviceState::default()
    };
    let idle = QuotaAdviceState {
        tool: Some(Tool::Codex),
        weekly_pct: Some(20.0),
        used_recently: true,
        ..QuotaAdviceState::default()
    };
    let report = m12_report(
        std::slice::from_ref(&usage),
        &[],
        &[],
        &[busy.clone(), idle.clone()],
        None,
    );
    let item = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "quota_rebalance")
        .expect("rebalance");
    assert_eq!(item.sessions, 0);
    assert_eq!(item.overhead_cost_usd.to_bits(), 0.0_f64.to_bits());
    assert_eq!(item.avoidable_cost_usd.to_bits(), 0.0_f64.to_bits());
    assert_eq!(item.consumed_weekly_pct, None);
    assert_eq!(item.avoidable_weekly_pct, None);
    let mut inactive = idle;
    inactive.used_recently = false;
    assert!(!ids(
        &m12_report(
            std::slice::from_ref(&usage),
            &[],
            &[],
            &[busy, inactive],
            None
        ),
        Tool::Claude
    )
    .contains(&"quota_rebalance".into()));

    let fit = QuotaAdviceState {
        tool: Some(Tool::Claude),
        plan: Some("max_5x".into()),
        completed_weekly_max: vec![10.0; 4],
        ..QuotaAdviceState::default()
    };
    let report = m12_report(
        std::slice::from_ref(&usage),
        &[],
        &[],
        std::slice::from_ref(&fit),
        None,
    );
    let item = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "plan_fit")
        .expect("plan fit");
    assert_eq!(item.sessions, 0);
    assert_eq!(item.avoidable_cost_usd, 0.0);
    assert!(item.facts.iter().any(|fact| fact.contains("Pro")));
    let mut too_high = fit;
    too_high.completed_weekly_max = vec![20.0; 4];
    assert!(!ids(
        &m12_report(std::slice::from_ref(&usage), &[], &[], &[too_high], None),
        Tool::Claude
    )
    .contains(&"plan_fit".into()));
    let codex_usage = event(
        Tool::Codex,
        "codex-plan",
        1,
        "gpt-6-sol",
        TokenCounts {
            input: 100,
            output: 10,
            ..TokenCounts::default()
        },
        0.0,
    );
    let plus_capped = QuotaAdviceState {
        tool: Some(Tool::Codex),
        plan: Some("plus".into()),
        completed_weekly_max: vec![100.0, 100.0, 50.0, 60.0],
        ..QuotaAdviceState::default()
    };
    assert!(ids(
        &m12_report(
            std::slice::from_ref(&codex_usage),
            &[],
            &[],
            &[plus_capped],
            None
        ),
        Tool::Codex
    )
    .contains(&"plan_fit".into()));
    let pro_unknown = QuotaAdviceState {
        tool: Some(Tool::Codex),
        plan: Some("pro".into()),
        completed_weekly_max: vec![10.0; 4],
        ..QuotaAdviceState::default()
    };
    assert!(!ids(
        &m12_report(&[codex_usage], &[], &[], &[pro_unknown], None),
        Tool::Codex
    )
    .contains(&"plan_fit".into()));

    let environment = EnvironmentSnapshot {
        claude_global_mcp_names: vec!["local".into()],
        claude_project_mcp_names: BTreeMap::from([
            (PathBuf::from("/fake/project"), vec!["current".into()]),
            (PathBuf::from("/another/project"), vec!["foreign".into()]),
        ]),
        claude_plugins: 2,
        claude_instruction_bytes: 9_000,
        ..EnvironmentSnapshot::default()
    };
    let report = m12_report(
        std::slice::from_ref(&usage),
        &[],
        &[],
        &[],
        Some(&environment),
    );
    let startup = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "session_startup")
        .expect("startup");
    assert!(startup.facts.iter().any(|fact| fact.contains("local")));
    assert!(startup
        .facts
        .iter()
        .any(|fact| fact.starts_with("本机配置的全局 MCP 服务器")));
    assert!(startup.facts.iter().any(|fact| fact == "启用 2 个插件"));
    assert!(startup
        .facts
        .iter()
        .any(|fact| fact == "项目级 MCP 服务器 1 个"));
    assert!(startup.facts.iter().all(|fact| !fact.contains("foreign")));
    assert!(startup.facts.iter().any(|fact| fact.contains("3000 token")));
    let mut no_local_plugins = environment.clone();
    no_local_plugins.claude_plugins = 0;
    let report = m12_report(
        std::slice::from_ref(&usage),
        &[],
        &[],
        &[],
        Some(&no_local_plugins),
    );
    let startup = report.tools[0]
        .items
        .iter()
        .find(|item| item.id == "session_startup")
        .expect("startup");
    assert!(startup.facts.iter().all(|fact| !fact.contains("插件")));
    assert!(startup.description.contains("账号同步"));
    let missing = m12_report(&[], &[], &[], &[], Some(&environment));
    assert!(!ids(&missing, Tool::Claude).contains(&"session_startup".into()));
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
    for minute in 61..99 {
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
