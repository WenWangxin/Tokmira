use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::attribution::{DimensionDelta, WindowKind};
use crate::claude_quota::WeeklyBreakdown;
use crate::dashboard::OverviewInput;
use crate::model::{Source, Tool};
use crate::timeline::{window_id, Origin, Window};

#[derive(Clone, Debug, Serialize)]
pub struct UsageWindowRef {
    pub tool: Tool,
    pub kind: WindowKind,
    pub window_id: DateTime<Utc>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub is_current: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageRank {
    pub key: String,
    pub name: String,
    pub subtitle: String,
    pub tokens: u64,
    pub requests: u64,
    pub delta: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsagePrompt {
    pub at: DateTime<Utc>,
    pub session_id: String,
    pub title: String,
    pub delta: f64,
    pub requests: u64,
    pub tokens: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageDetail {
    pub window: UsageWindowRef,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub resets_at: DateTime<Utc>,
    pub origin: Origin,
    pub attributed_total: f64,
    pub unattributed_total: f64,
    pub sessions: Vec<UsageRank>,
    pub projects: Vec<UsageRank>,
    pub models: Vec<UsageRank>,
    pub subagents: Vec<UsageRank>,
    pub high_prompts: Vec<UsagePrompt>,
    pub official_weekly_breakdown: Option<WeeklyBreakdown>,
}

fn selected(window: &crate::timeline::QuotaPoint, kind: WindowKind) -> Option<&Window> {
    match kind {
        WindowKind::FiveHour => window.five_hour.as_ref(),
        WindowKind::Weekly => window.weekly.as_ref(),
    }
}

/// Uses the same minimum observed reset and duration as attribution.
pub fn list_usage_windows(
    input: &OverviewInput<'_>,
    tool: Tool,
    now: DateTime<Utc>,
    limit_five_hour: usize,
) -> Vec<UsageWindowRef> {
    let mut groups: BTreeMap<(WindowKind, DateTime<Utc>), (DateTime<Utc>, u64)> = BTreeMap::new();
    for point in input
        .points
        .iter()
        .filter(|p| p.tool == tool && p.at <= now)
    {
        for kind in [WindowKind::FiveHour, WindowKind::Weekly] {
            let Some(w) = selected(point, kind) else {
                continue;
            };
            let (Some(id), Some(reset)) = (window_id(w), w.resets_at) else {
                continue;
            };
            let minutes = w.window_minutes.unwrap_or(match kind {
                WindowKind::FiveHour => 300,
                WindowKind::Weekly => 10080,
            });
            let entry = groups.entry((kind, id)).or_insert((reset, minutes));
            if reset < entry.0 {
                *entry = (reset, minutes);
            }
        }
    }
    let mut windows: Vec<_> = groups
        .into_iter()
        .filter_map(|((kind, id), (end, minutes))| {
            let start = end.checked_sub_signed(Duration::minutes(i64::try_from(minutes).ok()?))?;
            Some(UsageWindowRef {
                tool,
                kind,
                window_id: id,
                start,
                end,
                is_current: start <= now && now < end,
            })
        })
        .collect();
    windows.sort_by_key(|w| std::cmp::Reverse(w.end));
    let mut five_hours: Vec<_> = windows
        .iter()
        .filter(|w| w.kind == WindowKind::FiveHour)
        .take(limit_five_hour)
        .cloned()
        .collect();
    if let Some(weekly) = windows
        .into_iter()
        .filter(|w| w.kind == WindowKind::Weekly && w.is_current)
        .max_by_key(|w| w.end)
    {
        five_hours.push(weekly);
    }
    five_hours
}

pub(crate) fn directory_name(value: &str) -> String {
    value
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(value)
        .to_owned()
}

/// Untitled sessions (e.g. `codex exec`) often share a project, so the fallback
/// keeps the short session ID next to the project name to tell them apart.
pub(crate) fn session_display_name(id: &str, title: Option<&str>, project: Option<&str>) -> String {
    let short_id: String = id.chars().take(8).collect();
    title
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            project
                .filter(|path| !path.is_empty())
                .map(|path| format!("{} · {short_id}", directory_name(path)))
        })
        .unwrap_or(short_id)
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::ClaudeCli | Source::CodexCli => "命令行",
        Source::ClaudeDesktop | Source::CodexDesktop => "桌面端",
        Source::Unknown => "未知来源",
    }
}

fn rank(
    dimensions: &[DimensionDelta],
    counts: &BTreeMap<String, (u64, u64)>,
    label: impl Fn(&str, u64) -> (String, String),
) -> Vec<UsageRank> {
    let deltas: BTreeMap<_, _> = dimensions
        .iter()
        .map(|d| (d.key.as_str(), d.delta))
        .collect();
    let mut rows: Vec<_> = counts
        .iter()
        .map(|(key, (tokens, requests))| {
            let (name, subtitle) = label(key, *requests);
            UsageRank {
                key: key.clone(),
                name,
                subtitle,
                tokens: *tokens,
                requests: *requests,
                delta: deltas.get(key.as_str()).copied().unwrap_or(0.0),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.delta
            .total_cmp(&a.delta)
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| a.key.cmp(&b.key))
    });
    rows
}

pub fn usage_detail(
    input: &OverviewInput<'_>,
    window: &UsageWindowRef,
    now: DateTime<Utc>,
) -> UsageDetail {
    let matched = input.attribution.windows.iter().find(|w| {
        w.tool == window.tool && w.kind == window.kind && w.window_id == window.window_id
    });
    let empty: &[DimensionDelta] = &[];
    let by_session = matched.map_or(empty, |w| w.by_session.as_slice());
    let by_project = matched.map_or(empty, |w| w.by_project.as_slice());
    let by_model = matched.map_or(empty, |w| w.by_model.as_slice());
    let by_subagent = matched.map_or(empty, |w| w.by_subagent.as_slice());
    let observed = input
        .points
        .iter()
        .filter(|p| p.tool == window.tool && p.at <= now && p.at <= window.end)
        .filter_map(|p| Some((p, selected(p, window.kind)?)))
        .filter(|(_, w)| window_id(w) == Some(window.window_id) && w.used_percent.is_finite())
        .max_by_key(|(p, _)| p.at);
    let used_percent = observed.map_or(0.0, |(_, w)| w.used_percent).max(0.0);
    let origin = observed.map_or(Origin::Estimated, |(p, _)| p.origin);
    let mut session_counts = BTreeMap::new();
    let mut project_counts = BTreeMap::new();
    let mut model_counts = BTreeMap::new();
    let mut subagent_counts = BTreeMap::new();
    let mut session_meta: BTreeMap<String, (Option<String>, Source)> = BTreeMap::new();
    let mut project_sessions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for event in input.events.iter().filter(|e| {
        e.tool == window.tool
            && e.timestamp > window.start
            && e.timestamp <= window.end
            && e.timestamp <= now
    }) {
        let tokens = event.tokens.total_without_reasoning();
        let project = event.project.clone().unwrap_or_default();
        let agent = if event.is_subagent {
            "subagent"
        } else {
            "main"
        };
        for (map, key) in [
            (&mut session_counts, event.session_id.clone()),
            (&mut project_counts, project.clone()),
            (&mut model_counts, event.model.clone()),
            (&mut subagent_counts, agent.to_owned()),
        ] {
            let entry: &mut (u64, u64) = map.entry(key).or_default();
            entry.0 = entry.0.saturating_add(tokens);
            entry.1 = entry.1.saturating_add(1);
        }
        session_meta
            .entry(event.session_id.clone())
            .or_insert((event.project.clone(), event.source));
        project_sessions
            .entry(project)
            .or_default()
            .insert(event.session_id.clone());
    }
    let sessions = rank(by_session, &session_counts, |id, _| {
        let (project, source) = session_meta
            .get(id)
            .cloned()
            .unwrap_or((None, Source::Unknown));
        let name = session_display_name(
            id,
            input.titles.get(id).map(String::as_str),
            project.as_deref(),
        );
        let subtitle = format!(
            "{} · {}",
            project
                .as_deref()
                .map(directory_name)
                .unwrap_or_else(|| "未知项目".to_owned()),
            source_name(source)
        );
        (name, subtitle)
    });
    let projects = rank(by_project, &project_counts, |key, _| {
        (
            if key.is_empty() {
                "未知项目".to_owned()
            } else {
                directory_name(key)
            },
            format!(
                "{} 个会话",
                project_sessions.get(key).map_or(0, BTreeSet::len)
            ),
        )
    });
    let models = rank(by_model, &model_counts, |key, requests| {
        (key.to_owned(), format!("{requests} 次请求"))
    });
    let subagents = rank(by_subagent, &subagent_counts, |key, requests| {
        (
            if key == "subagent" {
                "子代理"
            } else {
                "主对话"
            }
            .to_owned(),
            format!("{requests} 次请求"),
        )
    });
    let mut high_prompts: Vec<_> = input
        .attribution
        .window_prompts
        .iter()
        .filter(|p| {
            p.tool == window.tool
                && p.kind == window.kind
                && p.window_id == window.window_id
                && p.delta > 0.0
        })
        .filter_map(|p| {
            let at = p.prompt_at?;
            if at <= window.start || at > window.end || at > now {
                return None;
            }
            let project = session_meta
                .get(&p.session_id)
                .and_then(|(project, _)| project.as_deref());
            Some(UsagePrompt {
                at,
                session_id: p.session_id.clone(),
                title: session_display_name(
                    &p.session_id,
                    p.title
                        .as_deref()
                        .or_else(|| input.titles.get(&p.session_id).map(String::as_str)),
                    project,
                ),
                delta: p.delta,
                requests: p.requests,
                tokens: p.tokens.total_without_reasoning(),
            })
        })
        .collect();
    high_prompts.sort_by(|a, b| {
        b.delta
            .total_cmp(&a.delta)
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| a.at.cmp(&b.at))
    });
    let official_weekly_breakdown =
        if window.tool == Tool::Claude && window.kind == WindowKind::Weekly && window.is_current {
            input
                .attribution
                .latest_claude_weekly_breakdown
                .clone()
                .filter(|b| {
                    b.window_started_at
                        .is_none_or(|start| (start - window.start).num_minutes().abs() <= 1)
                })
        } else {
            None
        };
    UsageDetail {
        window: window.clone(),
        used_percent,
        remaining_percent: (100.0 - used_percent).max(0.0),
        resets_at: window.end,
        origin,
        attributed_total: matched.map_or(0.0, |w| w.attributed_total),
        unattributed_total: matched.map_or(0.0, |w| w.unattributed_total),
        sessions,
        projects,
        models,
        subagents,
        high_prompts,
        official_weekly_breakdown,
    }
}
