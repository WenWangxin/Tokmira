//! Background reminders ("省额度提醒"): the advice rules, checked on every usage change so
//! the user hears about a costly habit while it is still fresh.
//!
//! Only problems that appear after the first check count: it marks everything already in
//! the logs as seen. Cache rebuilds are reported per request, the other rules once per
//! session. At most one reminder goes out per cooldown; problems found meanwhile are only
//! marked as seen. Session start-up is never reported, since it cannot be avoided.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::{
    alternative_family, context_tokens, format_tokens, model_family, tool_key, tool_rules,
    AdviceInput, Hit, RuleOutput,
};
use crate::model::{Tool, UsageEvent};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AdviceAlert {
    /// `<tool>:<rule>`, the key the advice page uses for dismissing.
    pub key: String,
    pub tool: Tool,
    pub session_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Default)]
pub struct AlertTracker {
    seen: HashSet<String>,
    seeded: bool,
    last_sent: Option<DateTime<Utc>>,
}

/// Most actionable first: a long context can still be fixed in the current session.
const PRIORITY: [&str; 4] = [
    "long_context",
    "cache_rebuild",
    "expensive_light_task",
    "many_subagents",
];

struct Candidate {
    id: String,
    rank: usize,
    /// False while a light task's prompt may still be running.
    settled: bool,
    alert: AdviceAlert,
}

impl AlertTracker {
    /// Marks every problem currently in the logs as seen without reporting it.
    pub fn seed(&mut self, input: &AdviceInput<'_>, now: DateTime<Utc>) {
        for candidate in candidates(input, now) {
            self.seen.insert(candidate.id);
        }
        self.seeded = true;
    }

    /// The reminder for problems that appeared since the last check, if one is due.
    pub fn check(&mut self, input: &AdviceInput<'_>, now: DateTime<Utc>) -> Option<AdviceAlert> {
        if !self.seeded {
            self.seed(input, now);
            return None;
        }
        let mut fresh: Vec<Candidate> = candidates(input, now)
            .into_iter()
            .filter(|candidate| candidate.settled && !self.seen.contains(&candidate.id))
            .collect();
        for candidate in &fresh {
            self.seen.insert(candidate.id.clone());
        }
        fresh.retain(|candidate| !input.dismissed.contains(&candidate.alert.key));
        let cooldown = Duration::minutes(input.config.alerts.cooldown_minutes);
        if self.last_sent.is_some_and(|sent| now - sent < cooldown) {
            return None;
        }
        fresh.sort_by_key(|candidate| candidate.rank);
        let alert = fresh.into_iter().next()?.alert;
        self.last_sent = Some(now);
        Some(alert)
    }
}

fn candidates(input: &AdviceInput<'_>, now: DateTime<Utc>) -> Vec<Candidate> {
    let since = now - Duration::hours(input.config.alerts.lookback_hours);
    let mut found = Vec::new();
    for tool in [Tool::Claude, Tool::Codex] {
        let Some(rules) = tool_rules(tool, input, since, now) else {
            continue;
        };
        for rule in &rules.rules {
            let Some(rank) = PRIORITY.iter().position(|id| *id == rule.id) else {
                continue;
            };
            let key = format!("{}:{}", tool_key(tool), rule.id);
            if rule.id == "cache_rebuild" {
                for hit in &rule.hits {
                    let event = &hit.item.event;
                    found.push(Candidate {
                        id: format!(
                            "{key}:{}:{}",
                            event.session_id,
                            event.timestamp.to_rfc3339()
                        ),
                        rank,
                        settled: true,
                        alert: alert(tool, &key, rule, event, cache_rebuild_body(input, hit)),
                    });
                }
                continue;
            }
            let mut by_session: BTreeMap<&str, Vec<&Hit<'_>>> = BTreeMap::new();
            for hit in &rule.hits {
                by_session
                    .entry(hit.item.event.session_id.as_str())
                    .or_default()
                    .push(hit);
            }
            for (session, hits) in by_session {
                let (settled, body) = match rule.id {
                    "long_context" => (true, long_context_body(&hits)),
                    "expensive_light_task" => light_task(input, tool, session, &hits, now),
                    _ => (true, subagents_body(&hits)),
                };
                found.push(Candidate {
                    id: format!("{key}:{session}"),
                    rank,
                    settled,
                    alert: alert(tool, &key, rule, &hits[0].item.event, body),
                });
            }
        }
    }
    found
}

fn alert(
    tool: Tool,
    key: &str,
    rule: &RuleOutput<'_>,
    event: &UsageEvent,
    body: String,
) -> AdviceAlert {
    let product = match tool {
        Tool::Claude => "Claude Code",
        Tool::Codex => "Codex",
    };
    // The project folder's name, not the session title: titles are derived from the
    // conversation, and notifications can show on the lock screen.
    let label = event
        .project
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| format!("「{}」", name.to_string_lossy()))
        .unwrap_or_default();
    AdviceAlert {
        key: key.to_owned(),
        tool,
        session_id: event.session_id.clone(),
        title: format!("{product} · {}", rule.title),
        body: format!("{label}{body}"),
    }
}

fn long_context_body(hits: &[&Hit<'_>]) -> String {
    let longest = hits
        .iter()
        .map(|hit| context_tokens(&hit.item.event.tokens))
        .max()
        .unwrap_or(0);
    format!(
        "当前上下文 {} token，每次请求都要整段重读。告一段落后可执行 /compact，或开启新会话。",
        format_tokens(longest)
    )
}

fn cache_rebuild_body(input: &AdviceInput<'_>, hit: &Hit<'_>) -> String {
    let event = &hit.item.event;
    let previous = input
        .events
        .iter()
        .map(|item| &item.event)
        .filter(|other| {
            other.tool == event.tool
                && !other.is_subagent
                && other.session_id == event.session_id
                && other.timestamp < event.timestamp
        })
        .map(|other| other.timestamp)
        .max();
    let gap = previous
        .map(|at| format!("隔了 {} 分钟才继续，", (event.timestamp - at).num_minutes()))
        .unwrap_or_default();
    format!(
        "{gap}缓存已过期，上下文重新写入了 {} token。连续的工作尽量一次做完，离开较久后可开启新会话。",
        format_tokens(hit.overhead_tokens)
    )
}

/// Judges a light prompt once it is over: the next prompt has started, or the session has
/// been quiet for the settle time.
fn light_task(
    input: &AdviceInput<'_>,
    tool: Tool,
    session: &str,
    hits: &[&Hit<'_>],
    now: DateTime<Utc>,
) -> (bool, String) {
    let prompts: Vec<DateTime<Utc>> = input
        .prompts
        .iter()
        .filter(|prompt| prompt.tool == tool && prompt.session_id == session)
        .map(|prompt| prompt.timestamp)
        .collect();
    let last_activity = input
        .events
        .iter()
        .map(|item| &item.event)
        .filter(|event| event.tool == tool && event.session_id == session)
        .map(|event| event.timestamp)
        .max();
    let quiet = last_activity.is_some_and(|at| {
        now - at >= Duration::minutes(input.config.alerts.light_task_settle_minutes)
    });
    // Each light prompt's requests, keyed by the prompt's time.
    let mut groups: BTreeMap<Option<DateTime<Utc>>, Vec<&Hit<'_>>> = BTreeMap::new();
    for hit in hits {
        let at = hit.item.event.timestamp;
        let prompt_at = prompts.iter().copied().filter(|start| *start <= at).max();
        groups.entry(prompt_at).or_default().push(hit);
    }
    // The latest light prompt that is over; a later light prompt still running does not
    // hold back one that already finished.
    let over = |start: &Option<DateTime<Utc>>| {
        quiet || start.is_some_and(|start| prompts.iter().any(|at| *at > start))
    };
    let empty = Vec::new();
    let (settled, group) = match groups.iter().rev().find(|(start, _)| over(start)) {
        Some((_, group)) => (true, group),
        None => (false, groups.values().next_back().unwrap_or(&empty)),
    };
    let output: u64 = group.iter().map(|hit| hit.item.event.tokens.output).sum();
    let family = group
        .first()
        .map(|hit| model_family(&hit.item.event.model))
        .unwrap_or_default();
    let advice = match tool {
        Tool::Claude => "这类任务先执行 /model sonnet 更省额度。".to_owned(),
        Tool::Codex => format!(
            "这类任务换用 {} 更省额度。",
            alternative_family(&input.config.light_task.codex_alternative)
        ),
    };
    (
        settled,
        format!("刚才的短任务只输出了 {output} token，却用了 {family}。{advice}"),
    )
}

fn subagents_body(hits: &[&Hit<'_>]) -> String {
    format!(
        "已发起 {} 次子代理请求，每个子代理都要重读一遍上下文。目标明确的小范围搜索，直接在主对话里做更省。",
        hits.len()
    )
}
