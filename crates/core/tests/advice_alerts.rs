use chrono::{DateTime, Duration, TimeZone, Utc};
use token_audit_core::advice::{AdviceAlert, AdviceConfig, AdviceInput, AlertTracker};
use token_audit_core::attribution::EventAttribution;
use token_audit_core::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use token_audit_core::pricing::Pricing;

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0)
        .single()
        .expect("fixed time")
}

fn at(minutes: i64) -> DateTime<Utc> {
    start() + Duration::minutes(minutes)
}

fn event(session: &str, minutes: i64, model: &str, tokens: TokenCounts) -> EventAttribution {
    EventAttribution {
        event: UsageEvent {
            tool: Tool::Claude,
            source: Source::ClaudeCli,
            session_id: session.to_owned(),
            request_id: None,
            timestamp: at(minutes),
            model: model.to_owned(),
            project: Some("/fake/project".to_owned()),
            is_subagent: false,
            tokens,
        },
        five_hour_delta: 0.0,
        weekly_delta: 0.0,
    }
}

fn tokens(cache_read: u64, write_5m: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input: 10,
        output,
        cache_read,
        cache_write_5m: write_5m,
        ..TokenCounts::default()
    }
}

fn prompt(session: &str, minutes: i64) -> PromptEvent {
    PromptEvent {
        tool: Tool::Claude,
        session_id: session.to_owned(),
        timestamp: at(minutes),
    }
}

struct Logs {
    events: Vec<EventAttribution>,
    prompts: Vec<PromptEvent>,
    dismissed: Vec<String>,
}

impl Logs {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            prompts: Vec::new(),
            dismissed: Vec::new(),
        }
    }

    fn check(&self, tracker: &mut AlertTracker, minutes: i64) -> Option<AdviceAlert> {
        let pricing = Pricing::bundled().expect("bundled pricing");
        let config = AdviceConfig::bundled().expect("bundled advice config");
        let input = AdviceInput {
            events: &self.events,
            prompts: &self.prompts,
            pricing: &pricing,
            config: &config,
            dismissed: &self.dismissed,
        };
        tracker.check(&input, at(minutes))
    }
}

/// A session that already has a long context before the app starts.
fn long_session(logs: &mut Logs, session: &str, minutes: i64) {
    logs.prompts.push(prompt(session, minutes));
    logs.events.push(event(
        session,
        minutes,
        "claude-sonnet-5-5",
        tokens(400_000, 0, 2_000),
    ));
}

#[test]
fn first_check_only_marks_existing_problems_as_seen() {
    let mut logs = Logs::new();
    long_session(&mut logs, "old", 0);
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 1), None);
    // The same long session keeps growing: still nothing, it was known at start.
    long_session(&mut logs, "old", 2);
    assert_eq!(logs.check(&mut tracker, 3), None);
}

#[test]
fn long_context_is_reported_once_per_session_with_project_name() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    long_session(&mut logs, "s", 1);
    let alert = logs.check(&mut tracker, 2).expect("long context alert");
    assert_eq!(alert.key, "claude:long_context");
    assert_eq!(alert.title, "Claude Code · 上下文过长");
    assert!(alert
        .body
        .starts_with("「project」当前上下文 40.0 万 token"));
    assert!(alert.body.contains("/compact"));
    long_session(&mut logs, "s", 20);
    assert_eq!(logs.check(&mut tracker, 21), None);
}

#[test]
fn cooldown_drops_problems_found_meanwhile() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    long_session(&mut logs, "a", 1);
    assert!(logs.check(&mut tracker, 1).is_some());
    long_session(&mut logs, "b", 2);
    assert_eq!(logs.check(&mut tracker, 2), None);
    // After the cooldown "b" is not reported late; a new problem is.
    assert_eq!(logs.check(&mut tracker, 10), None);
    long_session(&mut logs, "c", 11);
    assert_eq!(
        logs.check(&mut tracker, 11).map(|alert| alert.session_id),
        Some("c".to_owned())
    );
}

#[test]
fn dismissed_rules_stay_quiet() {
    let mut logs = Logs::new();
    logs.dismissed.push("claude:long_context".to_owned());
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    long_session(&mut logs, "s", 1);
    assert_eq!(logs.check(&mut tracker, 1), None);
}

#[test]
fn cache_rebuild_names_the_gap() {
    let mut logs = Logs::new();
    logs.prompts.push(prompt("s", 0));
    logs.events
        .push(event("s", 0, "claude-sonnet-5-5", tokens(0, 50_000, 3_000)));
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    logs.prompts.push(prompt("s", 12));
    logs.events.push(event(
        "s",
        12,
        "claude-sonnet-5-5",
        tokens(1_000, 52_000, 3_000),
    ));
    let alert = logs.check(&mut tracker, 12).expect("cache rebuild alert");
    assert_eq!(alert.key, "claude:cache_rebuild");
    assert!(alert
        .body
        .starts_with("「project」隔了 12 分钟才继续，缓存已过期，上下文重新写入了 5.2 万 token。"));
}

#[test]
fn light_task_waits_until_the_prompt_is_over() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    logs.prompts.push(prompt("s", 1));
    logs.events
        .push(event("s", 1, "claude-opus-5-5", tokens(40_000, 0, 120)));
    // The prompt may still be running.
    assert_eq!(logs.check(&mut tracker, 2), None);
    let alert = logs.check(&mut tracker, 5).expect("light task alert");
    assert_eq!(alert.title, "Claude Code · 轻量任务使用了 Opus");
    assert!(alert.body.contains("只输出了 120 token"));
    assert!(alert.body.contains("/model sonnet"));
}

#[test]
fn a_next_prompt_also_ends_the_light_task() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    logs.prompts.push(prompt("s", 1));
    logs.events
        .push(event("s", 1, "claude-opus-5-5", tokens(40_000, 0, 120)));
    logs.prompts.push(prompt("s", 2));
    assert!(logs.check(&mut tracker, 2).is_some());
}

#[test]
fn a_running_light_prompt_does_not_hold_back_a_finished_one() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    logs.prompts.push(prompt("s", 1));
    logs.events
        .push(event("s", 1, "claude-opus-5-5", tokens(40_000, 0, 46)));
    // The second short prompt may still be running, but the first one is over.
    logs.prompts.push(prompt("s", 2));
    logs.events
        .push(event("s", 2, "claude-opus-5-5", tokens(40_000, 0, 314)));
    let alert = logs.check(&mut tracker, 2).expect("light task alert");
    assert!(alert.body.contains("只输出了 46 token"));
}

#[test]
fn new_sessions_are_never_reported() {
    let mut logs = Logs::new();
    let mut tracker = AlertTracker::default();
    assert_eq!(logs.check(&mut tracker, 0), None);
    logs.prompts.push(prompt("fresh", 1));
    logs.events.push(event(
        "fresh",
        1,
        "claude-sonnet-5-5",
        tokens(0, 30_000, 2_000),
    ));
    assert_eq!(logs.check(&mut tracker, 10), None);
}
