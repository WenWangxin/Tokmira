use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::claude_quota::WeeklyBreakdown;
use crate::model::{PromptEvent, Source, TokenCounts, Tool, UsageEvent};
use crate::pricing::Pricing;
use crate::quota_store::ClaudeSnapshot;
use crate::timeline::{window_id, QuotaPoint, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WindowKind {
    FiveHour,
    Weekly,
}
#[derive(Clone, Debug, Serialize)]
pub struct EventAttribution {
    pub event: UsageEvent,
    pub five_hour_delta: f64,
    pub weekly_delta: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct DimensionDelta {
    pub key: String,
    pub delta: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct WindowAttribution {
    pub tool: Tool,
    pub kind: WindowKind,
    pub window_id: DateTime<Utc>,
    pub points: usize,
    pub increment_total: f64,
    pub attributed_total: f64,
    pub unattributed_total: f64,
    pub decreases: u64,
    pub by_session: Vec<DimensionDelta>,
    pub by_project: Vec<DimensionDelta>,
    pub by_model: Vec<DimensionDelta>,
    pub by_source: Vec<DimensionDelta>,
    pub by_subagent: Vec<DimensionDelta>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PromptAttribution {
    pub tool: Tool,
    pub session_id: String,
    pub title: Option<String>,
    pub prompt_at: Option<DateTime<Utc>>,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub five_hour_delta: f64,
    pub weekly_delta: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Attribution {
    pub events: Vec<EventAttribution>,
    pub windows: Vec<WindowAttribution>,
    pub prompts: Vec<PromptAttribution>,
    pub window_prompts: Vec<WindowPromptAttribution>,
    pub latest_claude_weekly_breakdown: Option<WeeklyBreakdown>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WindowPromptAttribution {
    pub tool: Tool,
    pub kind: WindowKind,
    pub window_id: DateTime<Utc>,
    pub session_id: String,
    pub title: Option<String>,
    pub prompt_at: Option<DateTime<Utc>>,
    pub delta: f64,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
}
impl Attribution {
    pub fn top_prompts(
        &self,
        tool: Tool,
        kind: WindowKind,
        window_id: DateTime<Utc>,
        limit: usize,
    ) -> Vec<&WindowPromptAttribution> {
        let mut result: Vec<_> = self
            .window_prompts
            .iter()
            .filter(|p| {
                p.tool == tool
                    && p.kind == kind
                    && p.window_id == window_id
                    && p.prompt_at.is_some()
            })
            .collect();
        result.sort_by(|a, b| {
            b.delta
                .total_cmp(&a.delta)
                .then_with(|| a.session_id.cmp(&b.session_id))
                .then_with(|| a.prompt_at.cmp(&b.prompt_at))
        });
        result.truncate(limit);
        result
    }
}

#[derive(Default)]
struct WindowWork {
    range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    points: usize,
    increment_total: f64,
    attributed_total: f64,
    unattributed_total: f64,
    decreases: u64,
    per_event: BTreeMap<usize, f64>,
}

fn selected(point: &QuotaPoint, kind: WindowKind) -> Option<&Window> {
    match kind {
        WindowKind::FiveHour => point.five_hour.as_ref(),
        WindowKind::Weekly => point.weekly.as_ref(),
    }
}
fn add_dimension(map: &mut BTreeMap<String, f64>, key: String, delta: f64) {
    *map.entry(key).or_default() += delta;
}
fn sorted_dimensions(map: BTreeMap<String, f64>) -> Vec<DimensionDelta> {
    let mut result: Vec<_> = map
        .into_iter()
        .map(|(key, delta)| DimensionDelta { key, delta })
        .collect();
    result.sort_by(|a, b| b.delta.total_cmp(&a.delta).then_with(|| a.key.cmp(&b.key)));
    result
}

/// Attribution is deterministic: points and events are sorted by timestamp with their
/// original position as a tie breaker; intervals and event weights are summed in that
/// order. Each reset window starts at 0%. Only positive observed changes are allocated.
/// Unpriced requests receive zero weight when a priced request exists in the interval;
/// if no positive price is available, token volume supplies the weights instead.
/// The final weighted event receives the arithmetic remainder to preserve totals.
pub fn attribute(
    events: &[UsageEvent],
    prompts: &[PromptEvent],
    titles: &BTreeMap<String, String>,
    points: &[QuotaPoint],
    claude_snapshots: &[ClaudeSnapshot],
    pricing: &Pricing,
) -> Attribution {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&index| (events[index].timestamp, index));
    let mut event_deltas = vec![(0.0, 0.0); events.len()];
    let mut work: BTreeMap<(Tool, WindowKind, DateTime<Utc>), WindowWork> = BTreeMap::new();
    for tool in [Tool::Claude, Tool::Codex] {
        for kind in [WindowKind::FiveHour, WindowKind::Weekly] {
            let mut groups: BTreeMap<DateTime<Utc>, Vec<(usize, &QuotaPoint, &Window)>> =
                BTreeMap::new();
            for (index, point) in points.iter().enumerate() {
                if point.tool != tool {
                    continue;
                }
                let Some(window) = selected(point, kind) else {
                    continue;
                };
                if !window.used_percent.is_finite() {
                    continue;
                }
                if let Some(id) = window_id(window) {
                    groups.entry(id).or_default().push((index, point, window));
                }
            }
            for (id, mut group) in groups {
                group.sort_by_key(|(index, point, _)| (point.at, *index));
                let minutes = group[0].2.window_minutes.unwrap_or(match kind {
                    WindowKind::FiveHour => 300,
                    WindowKind::Weekly => 10080,
                });
                let Some(reset) = group.iter().filter_map(|(_, _, w)| w.resets_at).min() else {
                    continue;
                };
                let Some(start) = reset.checked_sub_signed(Duration::minutes(minutes as i64))
                else {
                    continue;
                };
                let item = work.entry((tool, kind, id)).or_default();
                item.points = group.len();
                item.range = group.last().map(|(_, point, _)| (start, point.at));
                let mut prev_at = start;
                let mut prev_percent = 0.0;
                for (_, point, window) in group {
                    let delta = window.used_percent - prev_percent;
                    if delta < 0.0 {
                        item.decreases += 1;
                        // A lower reading from a conflicting source starts a new baseline.
                        prev_at = point.at;
                        prev_percent = window.used_percent;
                        continue;
                    }
                    // Integer percentages may remain unchanged across several requests.
                    // Keep the interval open until a positive change can cover them all.
                    if delta == 0.0 {
                        continue;
                    }
                    if delta > 0.0 {
                        item.increment_total += delta;
                        let eligible: Vec<usize> = order
                            .iter()
                            .copied()
                            .filter(|&i| {
                                events[i].tool == tool
                                    && events[i].timestamp > prev_at
                                    && events[i].timestamp <= point.at
                            })
                            .collect();
                        let priced: Vec<f64> = eligible
                            .iter()
                            .map(|&i| {
                                pricing
                                    .cost_usd(&events[i].model, &events[i].tokens)
                                    .unwrap_or(0.0)
                                    .max(0.0)
                            })
                            .collect();
                        let priced_sum: f64 = priced.iter().copied().sum();
                        let weights: Vec<f64> = if priced_sum > 0.0 {
                            priced
                        } else {
                            eligible
                                .iter()
                                .map(|&i| events[i].tokens.total_without_reasoning() as f64)
                                .collect()
                        };
                        let weight_sum: f64 = weights.iter().copied().sum();
                        if weight_sum == 0.0 {
                            item.unattributed_total += delta;
                        } else {
                            let last_weighted = weights.iter().rposition(|weight| *weight > 0.0);
                            let mut allocated = 0.0;
                            for (position, (&index, &weight)) in
                                eligible.iter().zip(&weights).enumerate()
                            {
                                if weight <= 0.0 {
                                    continue;
                                }
                                let share = if Some(position) == last_weighted {
                                    delta - allocated
                                } else {
                                    delta * weight / weight_sum
                                };
                                allocated += share;
                                item.attributed_total += share;
                                *item.per_event.entry(index).or_default() += share;
                                match kind {
                                    WindowKind::FiveHour => event_deltas[index].0 += share,
                                    WindowKind::Weekly => event_deltas[index].1 += share,
                                }
                            }
                        }
                    }
                    if delta > 0.0 {
                        prev_at = point.at;
                        prev_percent = window.used_percent;
                    }
                }
            }
        }
    }
    let mut prompt_order: BTreeMap<(Tool, String), Vec<DateTime<Utc>>> = BTreeMap::new();
    for prompt in prompts {
        prompt_order
            .entry((prompt.tool, prompt.session_id.clone()))
            .or_default()
            .push(prompt.timestamp);
    }
    for times in prompt_order.values_mut() {
        times.sort();
        times.dedup();
    }
    let mut window_prompts = Vec::new();
    let windows = work
        .into_iter()
        .map(|((tool, kind, id), item)| {
            let mut by_session = BTreeMap::new();
            let mut by_project = BTreeMap::new();
            let mut by_model = BTreeMap::new();
            let mut by_source = BTreeMap::new();
            let mut by_subagent = BTreeMap::new();
            let mut prompt_groups: BTreeMap<
                (String, Option<DateTime<Utc>>),
                WindowPromptAttribution,
            > = BTreeMap::new();
            for &index in &order {
                let event = &events[index];
                let Some((start, end)) = item.range else {
                    continue;
                };
                if event.tool != tool || event.timestamp <= start || event.timestamp > end {
                    continue;
                }
                let delta = item.per_event.get(&index).copied().unwrap_or(0.0);
                if delta > 0.0 {
                    add_dimension(&mut by_session, event.session_id.clone(), delta);
                    add_dimension(
                        &mut by_project,
                        event.project.clone().unwrap_or_default(),
                        delta,
                    );
                    add_dimension(&mut by_model, event.model.clone(), delta);
                    add_dimension(&mut by_source, format!("{:?}", event.source), delta);
                    add_dimension(
                        &mut by_subagent,
                        if event.is_subagent {
                            "subagent"
                        } else {
                            "main"
                        }
                        .to_owned(),
                        delta,
                    );
                }
                let prompt_at = prompt_order
                    .get(&(event.tool, event.session_id.clone()))
                    .and_then(|times| {
                        times
                            .partition_point(|&time| time <= event.timestamp)
                            .checked_sub(1)
                            .map(|pos| times[pos])
                    });
                let prompt = prompt_groups
                    .entry((event.session_id.clone(), prompt_at))
                    .or_insert_with(|| WindowPromptAttribution {
                        tool,
                        kind,
                        window_id: id,
                        session_id: event.session_id.clone(),
                        title: titles.get(&event.session_id).cloned(),
                        prompt_at,
                        delta: 0.0,
                        requests: 0,
                        tokens: TokenCounts::default(),
                        first_at: event.timestamp,
                        last_at: event.timestamp,
                    });
                prompt.delta += delta;
                prompt.requests += 1;
                prompt.tokens.add_saturating(&event.tokens);
                prompt.first_at = prompt.first_at.min(event.timestamp);
                prompt.last_at = prompt.last_at.max(event.timestamp);
            }
            window_prompts.extend(prompt_groups.into_values());
            WindowAttribution {
                tool,
                kind,
                window_id: id,
                points: item.points,
                increment_total: item.increment_total,
                attributed_total: item.attributed_total,
                unattributed_total: item.unattributed_total,
                decreases: item.decreases,
                by_session: sorted_dimensions(by_session),
                by_project: sorted_dimensions(by_project),
                by_model: sorted_dimensions(by_model),
                by_source: sorted_dimensions(by_source),
                by_subagent: sorted_dimensions(by_subagent),
            }
        })
        .collect();
    let mut groups: BTreeMap<(Tool, String, Option<DateTime<Utc>>), PromptAttribution> =
        BTreeMap::new();
    for index in order {
        let event = &events[index];
        let prompt_at = prompt_order
            .get(&(event.tool, event.session_id.clone()))
            .and_then(|times| {
                times
                    .partition_point(|&time| time <= event.timestamp)
                    .checked_sub(1)
                    .map(|pos| times[pos])
            });
        let key = (event.tool, event.session_id.clone(), prompt_at);
        let group = groups.entry(key).or_insert_with(|| PromptAttribution {
            tool: event.tool,
            session_id: event.session_id.clone(),
            title: titles.get(&event.session_id).cloned(),
            prompt_at,
            first_at: event.timestamp,
            last_at: event.timestamp,
            requests: 0,
            tokens: TokenCounts::default(),
            five_hour_delta: 0.0,
            weekly_delta: 0.0,
        });
        group.first_at = group.first_at.min(event.timestamp);
        group.last_at = group.last_at.max(event.timestamp);
        group.requests += 1;
        group.tokens.add_saturating(&event.tokens);
        group.five_hour_delta += event_deltas[index].0;
        group.weekly_delta += event_deltas[index].1;
    }
    let latest_claude_weekly_breakdown = claude_snapshots
        .iter()
        .max_by_key(|s| s.at)
        .and_then(|s| s.weekly_breakdown.clone());
    Attribution {
        events: events
            .iter()
            .cloned()
            .zip(event_deltas)
            .map(
                |(event, (five_hour_delta, weekly_delta))| EventAttribution {
                    event,
                    five_hour_delta,
                    weekly_delta,
                },
            )
            .collect(),
        windows,
        prompts: groups.into_values().collect(),
        window_prompts,
        latest_claude_weekly_breakdown,
    }
}

pub fn source_label(source: Source) -> &'static str {
    match source {
        Source::ClaudeCli => "Claude Code · 命令行",
        Source::ClaudeDesktop => "Claude · 桌面版",
        Source::CodexCli => "Codex · 命令行",
        Source::CodexDesktop => "Codex · 桌面版",
        Source::Unknown => "未知来源",
    }
}
