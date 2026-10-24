//! Session list and request timeline.
//!
//! Counts and titles come from usage events and the existing title map. No message body
//! is read. Weekly quota shares are the attribution deltas already stored on each event.

use std::collections::{BTreeMap, VecDeque};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::advice::{EventTag, Severity};
use crate::attribution::EventAttribution;
use crate::model::{PromptEvent, Source, TokenCounts, Tool};
use crate::pricing::Pricing;
use crate::usage::{directory_name, session_display_name};

pub struct SessionInput<'a> {
    pub events: &'a [EventAttribution],
    pub titles: &'a BTreeMap<String, String>,
    pub pricing: &'a Pricing,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionRow {
    pub tool: Tool,
    pub session_id: String,
    pub title: Option<String>,
    pub display_name: String,
    pub project: Option<String>,
    pub project_name: String,
    pub source: Source,
    pub model: String,
    pub requests: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub estimated_cost_usd: f64,
    pub weekly_delta: f64,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    /// True when this tool has any positive weekly attribution inside the list window.
    pub has_quota_data: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimelineTag {
    pub tag: String,
    pub severity: Severity,
}

/// A prompt is ordered before a request that happened at the same instant.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind")]
pub enum TimelineEntry {
    Prompt {
        at: DateTime<Utc>,
        gap_minutes_from_previous_prompt: Option<i64>,
    },
    Request {
        at: DateTime<Utc>,
        index: u64,
        is_subagent: bool,
        model: String,
        tokens: TokenCounts,
        weekly_delta: f64,
        tag: Option<TimelineTag>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionDetail {
    pub tool: Tool,
    pub session_id: String,
    pub title: Option<String>,
    pub display_name: String,
    pub project: Option<String>,
    pub project_name: String,
    pub source: Source,
    pub model: String,
    pub requests: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub estimated_cost_usd: f64,
    pub weekly_delta: f64,
    /// True when this tool has any positive weekly attribution in `input.events`.
    pub has_quota_data: bool,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    pub entries: Vec<TimelineEntry>,
}

struct Facts {
    title: Option<String>,
    display_name: String,
    project: Option<String>,
    project_name: String,
    source: Source,
    model: String,
    requests: u64,
    total_tokens: u64,
    cache_hit_rate: Option<f64>,
    estimated_cost_usd: f64,
    weekly_delta: f64,
    first_at: DateTime<Utc>,
    last_at: DateTime<Utc>,
}

fn cache_hit_rate(tokens: &TokenCounts) -> Option<f64> {
    let denominator = tokens.input as u128
        + tokens.cache_read as u128
        + tokens.cache_write_5m as u128
        + tokens.cache_write_1h as u128
        + tokens.cache_write_unspecified as u128;
    (denominator > 0).then(|| tokens.cache_read as f64 / denominator as f64)
}

fn title_of(input: &SessionInput<'_>, session_id: &str) -> Option<String> {
    input
        .titles
        .get(session_id)
        .filter(|title| !title.trim().is_empty())
        .cloned()
}

/// Higher count wins. Equal counts keep the earlier timestamp, then the smaller key.
fn dominant<T: Clone + Ord>(items: impl IntoIterator<Item = (T, DateTime<Utc>)>) -> Option<T> {
    let mut counts: BTreeMap<T, (u64, DateTime<Utc>)> = BTreeMap::new();
    for (key, at) in items {
        let entry = counts.entry(key).or_insert((0, at));
        entry.0 += 1;
        entry.1 = entry.1.min(at);
    }
    counts
        .into_iter()
        .max_by(|left, right| {
            let (left_count, left_at) = left.1;
            let (right_count, right_at) = right.1;
            left_count
                .cmp(&right_count)
                .then(right_at.cmp(&left_at))
                .then(right.0.cmp(&left.0))
        })
        .map(|(value, _)| value)
}

fn facts(
    input: &SessionInput<'_>,
    session_id: &str,
    events: &[&EventAttribution],
) -> Option<Facts> {
    let mut tokens = TokenCounts::default();
    let mut estimated_cost_usd = 0.0;
    let mut weekly_delta = 0.0;
    let mut first_at = events.first()?.event.timestamp;
    let mut last_at = first_at;
    for event in events {
        tokens.add_saturating(&event.event.tokens);
        estimated_cost_usd += input
            .pricing
            .cost_usd(&event.event.model, &event.event.tokens)
            .unwrap_or(0.0);
        weekly_delta += event.weekly_delta;
        first_at = first_at.min(event.event.timestamp);
        last_at = last_at.max(event.event.timestamp);
    }
    let project = dominant(events.iter().map(|event| {
        (
            event
                .event
                .project
                .as_deref()
                .filter(|path| !path.is_empty())
                .map(str::to_owned),
            event.event.timestamp,
        )
    }))
    .flatten();
    let title = title_of(input, session_id);
    Some(Facts {
        display_name: session_display_name(session_id, title.as_deref(), project.as_deref()),
        title,
        project_name: project.as_deref().map(directory_name).unwrap_or_default(),
        project,
        source: dominant(
            events
                .iter()
                .map(|event| (event.event.source, event.event.timestamp)),
        )
        .unwrap_or(Source::Unknown),
        model: dominant(
            events
                .iter()
                .map(|event| (event.event.model.clone(), event.event.timestamp)),
        )
        .unwrap_or_default(),
        requests: u64::try_from(events.len()).unwrap_or(u64::MAX),
        total_tokens: tokens.total_without_reasoning(),
        cache_hit_rate: cache_hit_rate(&tokens),
        estimated_cost_usd,
        weekly_delta,
        first_at,
        last_at,
    })
}

fn window_start(now: DateTime<Utc>, days: u32) -> DateTime<Utc> {
    now.checked_sub_signed(Duration::days(i64::from(days)))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Sessions with at least one request in `[now - days, now]`, both tools included.
/// Ordered by weekly attribution, then estimated cost, both descending.
pub fn list_sessions(input: &SessionInput<'_>, days: u32, now: DateTime<Utc>) -> Vec<SessionRow> {
    let since = window_start(now, days);
    let period: Vec<&EventAttribution> = input
        .events
        .iter()
        .filter(|item| item.event.timestamp >= since && item.event.timestamp <= now)
        .collect();
    let has_quota: BTreeMap<Tool, bool> = [Tool::Claude, Tool::Codex]
        .into_iter()
        .map(|tool| {
            (
                tool,
                period
                    .iter()
                    .any(|item| item.event.tool == tool && item.weekly_delta > 0.0),
            )
        })
        .collect();
    let mut groups: BTreeMap<(Tool, &str), Vec<&EventAttribution>> = BTreeMap::new();
    for item in &period {
        groups
            .entry((item.event.tool, item.event.session_id.as_str()))
            .or_default()
            .push(*item);
    }
    let mut rows = Vec::new();
    for ((tool, session_id), events) in groups {
        let Some(facts) = facts(input, session_id, &events) else {
            continue;
        };
        rows.push(SessionRow {
            tool,
            session_id: session_id.to_owned(),
            title: facts.title,
            display_name: facts.display_name,
            project: facts.project,
            project_name: facts.project_name,
            source: facts.source,
            model: facts.model,
            requests: facts.requests,
            total_tokens: facts.total_tokens,
            cache_hit_rate: facts.cache_hit_rate,
            estimated_cost_usd: facts.estimated_cost_usd,
            weekly_delta: facts.weekly_delta,
            first_at: facts.first_at,
            last_at: facts.last_at,
            has_quota_data: has_quota.get(&tool).copied().unwrap_or(false),
        });
    }
    rows.sort_by(|left, right| {
        right
            .weekly_delta
            .total_cmp(&left.weekly_delta)
            .then(right.estimated_cost_usd.total_cmp(&left.estimated_cost_usd))
            .then(left.tool.cmp(&right.tool))
            .then(left.session_id.cmp(&right.session_id))
    });
    rows
}

fn timeline(
    events: &[&EventAttribution],
    prompts: &[PromptEvent],
    tags: &[EventTag],
    tool: Tool,
    session_id: &str,
) -> Vec<TimelineEntry> {
    let mut prompt_times: Vec<DateTime<Utc>> = prompts
        .iter()
        .filter(|prompt| prompt.tool == tool && prompt.session_id == session_id)
        .map(|prompt| prompt.timestamp)
        .collect();
    prompt_times.sort();
    prompt_times.dedup();
    let mut requests = events.to_vec();
    requests.sort_by(|left, right| {
        left.event
            .timestamp
            .cmp(&right.event.timestamp)
            .then(left.event.request_id.cmp(&right.event.request_id))
    });
    // A request id pins the tag to that request. Tags without one stay in emission
    // order and only fill requests that also have no id, so a tagged neighbor
    // cannot pick up a tag that belongs to someone else.
    let mut identified: BTreeMap<DateTime<Utc>, BTreeMap<String, VecDeque<&EventTag>>> =
        BTreeMap::new();
    let mut anonymous: BTreeMap<DateTime<Utc>, VecDeque<&EventTag>> = BTreeMap::new();
    for tag in tags
        .iter()
        .filter(|tag| tag.tool == tool && tag.session_id == session_id)
    {
        match &tag.request_id {
            Some(id) => identified
                .entry(tag.at)
                .or_default()
                .entry(id.clone())
                .or_default()
                .push_back(tag),
            None => anonymous.entry(tag.at).or_default().push_back(tag),
        }
    }

    let mut entries = Vec::new();
    let mut previous_prompt: Option<DateTime<Utc>> = None;
    for at in prompt_times {
        let gap = previous_prompt.map(|previous| at.signed_duration_since(previous).num_minutes());
        previous_prompt = Some(at);
        entries.push((
            at,
            0_u8,
            TimelineEntry::Prompt {
                at,
                gap_minutes_from_previous_prompt: gap,
            },
        ));
    }
    for (offset, event) in requests.iter().enumerate() {
        let at = event.event.timestamp;
        let tag = match &event.event.request_id {
            Some(id) => identified
                .get_mut(&at)
                .and_then(|by_id| by_id.get_mut(id))
                .and_then(VecDeque::pop_front),
            None => anonymous.get_mut(&at).and_then(VecDeque::pop_front),
        }
        .map(|tag| TimelineTag {
            tag: tag.tag.clone(),
            severity: tag.severity,
        });
        entries.push((
            at,
            1_u8,
            TimelineEntry::Request {
                at,
                index: u64::try_from(offset).unwrap_or(u64::MAX - 1) + 1,
                is_subagent: event.event.is_subagent,
                model: event.event.model.clone(),
                tokens: event.event.tokens,
                weekly_delta: event.weekly_delta,
                tag,
            },
        ));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    entries.into_iter().map(|(_, _, entry)| entry).collect()
}

/// Full history of one session. `None` when that tool has no requests for the id.
/// Prompt gaps are whole minutes; the UI decides which gaps to mention.
pub fn session_detail(
    input: &SessionInput<'_>,
    prompts: &[PromptEvent],
    advice_event_tags: &[EventTag],
    tool: Tool,
    session_id: &str,
) -> Option<SessionDetail> {
    let events: Vec<&EventAttribution> = input
        .events
        .iter()
        .filter(|item| item.event.tool == tool && item.event.session_id == session_id)
        .collect();
    let facts = facts(input, session_id, &events)?;
    let has_quota_data = input
        .events
        .iter()
        .any(|item| item.event.tool == tool && item.weekly_delta > 0.0);
    Some(SessionDetail {
        tool,
        session_id: session_id.to_owned(),
        title: facts.title,
        display_name: facts.display_name,
        project: facts.project,
        project_name: facts.project_name,
        source: facts.source,
        model: facts.model,
        requests: facts.requests,
        total_tokens: facts.total_tokens,
        cache_hit_rate: facts.cache_hit_rate,
        estimated_cost_usd: facts.estimated_cost_usd,
        weekly_delta: facts.weekly_delta,
        has_quota_data,
        first_at: facts.first_at,
        last_at: facts.last_at,
        entries: timeline(&events, prompts, advice_event_tags, tool, session_id),
    })
}
