use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use serde::Serialize;

use crate::attribution::{Attribution, WindowKind as AttributionWindowKind};
use crate::codex::RateLimitSnapshot;
use crate::forecast::{forecast, Forecast, WindowKind};
use crate::model::{Source, TokenCounts, Tool, UsageEvent};
use crate::pricing::Pricing;
use crate::quota_store::{ClaudeSnapshot, CodexSnapshot};
use crate::recorder::RecorderStatus;
use crate::timeline::{window_id, Origin, QuotaPoint};

pub struct OverviewInput<'a> {
    pub events: &'a [UsageEvent],
    pub titles: &'a BTreeMap<String, String>,
    pub points: &'a [QuotaPoint],
    pub attribution: &'a Attribution,
    pub claude_snapshots: &'a [ClaudeSnapshot],
    pub codex_snapshots: &'a [CodexSnapshot],
    pub codex_log: &'a [RateLimitSnapshot],
    pub pricing: &'a Pricing,
    pub recorder: RecorderStatus,
}
#[derive(Clone, Debug, Serialize)]
pub struct QuotaDisplay {
    pub used_percent: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub origin: Option<Origin>,
    pub forecast: Forecast,
    pub window_passed: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolOverview {
    pub tool: Tool,
    pub plan: Option<String>,
    pub five_hour: QuotaDisplay,
    pub weekly: QuotaDisplay,
}
#[derive(Clone, Debug, Serialize)]
pub struct TrendPoint {
    pub at: DateTime<Utc>,
    pub percent: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct PromptMarker {
    pub at: DateTime<Utc>,
    pub delta: f64,
    pub session_id: String,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolTrend {
    pub tool: Tool,
    pub five_hour: Vec<TrendPoint>,
    pub high_prompts: Vec<PromptMarker>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DayCost {
    pub date: NaiveDate,
    pub estimated_cost_usd: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct SessionOverview {
    pub tool: Tool,
    pub session_id: String,
    pub title: Option<String>,
    pub source: Source,
    pub project: Option<String>,
    pub model: String,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub cache_hit_rate: Option<f64>,
    /// Percentage points attributed to this session in the current five-hour window only.
    pub five_hour_delta: f64,
    /// Sum of percentage points across weekly windows; each share is relative to its own week.
    pub weekly_delta: f64,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub sessions: usize,
    pub requests: usize,
    /// Recent seven-day prompt count with >=0.5 weekly points or >=1 point in the current five-hour window.
    pub high_prompts: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub generated_at: DateTime<Utc>,
    pub tools: Vec<ToolOverview>,
    pub today_tokens: TokenCounts,
    pub today_cache_read_share: Option<f64>,
    pub peak_hour: Option<u32>,
    pub hourly_tokens: [u64; 24],
    pub today_cache_hit_rate: Option<f64>,
    pub cache_hit_change_from_yesterday: Option<f64>,
    pub month_estimated_cost_usd: f64,
    pub daily_cost: Vec<DayCost>,
    pub recorder: RecorderStatus,
    pub trends: Vec<ToolTrend>,
    pub recent_sessions: Vec<SessionOverview>,
    pub summary: Summary,
}
fn cache_hit(tokens: &TokenCounts) -> Option<f64> {
    let denominator = tokens.input as u128
        + tokens.cache_read as u128
        + tokens.cache_write_5m as u128
        + tokens.cache_write_1h as u128
        + tokens.cache_write_unspecified as u128;
    (denominator > 0).then(|| tokens.cache_read as f64 / denominator as f64)
}
fn display(
    points: &[QuotaPoint],
    tool: Tool,
    kind: WindowKind,
    now: DateTime<Utc>,
) -> QuotaDisplay {
    let latest = points
        .iter()
        .filter(|p| p.tool == tool && p.at <= now)
        .filter_map(|p| {
            let window = match kind {
                WindowKind::FiveHour => p.five_hour.as_ref(),
                WindowKind::Weekly => p.weekly.as_ref(),
            }?;
            Some((p, window))
        })
        .max_by_key(|(p, _)| p.at);
    let window_passed = latest
        .and_then(|(_, w)| w.resets_at)
        .is_some_and(|reset| reset <= now);
    QuotaDisplay {
        used_percent: latest
            .filter(|_| !window_passed)
            .map(|(_, w)| w.used_percent),
        resets_at: latest.and_then(|(_, w)| w.resets_at),
        origin: Some(latest.map_or(Origin::Estimated, |(p, _)| p.origin)),
        forecast: forecast(points, tool, kind, now),
        window_passed,
    }
}
fn current_five_hour_id(
    points: &[QuotaPoint],
    tool: Tool,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    points
        .iter()
        .filter(|p| p.tool == tool && p.at <= now)
        .filter_map(|p| Some((p.at, p.five_hour.as_ref()?)))
        .max_by_key(|(at, _)| *at)
        .and_then(|(_, window)| {
            (window.resets_at? > now)
                .then(|| window_id(window))
                .flatten()
        })
}
fn current_five_hour(points: &[QuotaPoint], tool: Tool, now: DateTime<Utc>) -> Vec<TrendPoint> {
    let Some(id) = current_five_hour_id(points, tool, now) else {
        return Vec::new();
    };
    let mut result: Vec<_> = points
        .iter()
        .filter(|p| p.tool == tool && p.at <= now)
        .filter_map(|p| {
            let w = p.five_hour.as_ref()?;
            (window_id(w) == Some(id)).then_some(TrendPoint {
                at: p.at,
                percent: w.used_percent,
            })
        })
        .collect();
    result.sort_by_key(|p| p.at);
    result
}
fn prompt_markers(input: &OverviewInput<'_>, tool: Tool, now: DateTime<Utc>) -> Vec<PromptMarker> {
    let id = current_five_hour_id(input.points, tool, now);
    let mut result: Vec<_> = input
        .attribution
        .window_prompts
        .iter()
        .filter(|p| {
            p.tool == tool
                && p.kind == AttributionWindowKind::FiveHour
                && Some(p.window_id) == id
                && p.delta >= 1.0
        })
        .filter_map(|p| {
            Some(PromptMarker {
                at: p.prompt_at.filter(|at| *at <= now)?,
                delta: p.delta,
                session_id: p.session_id.clone(),
                title: p.title.clone(),
            })
        })
        .collect();
    result.sort_by_key(|p| p.at);
    result
}

pub fn build_overview<Tz: TimeZone>(
    input: &OverviewInput<'_>,
    now: DateTime<Utc>,
    tz: Tz,
) -> Overview {
    let today = now.with_timezone(&tz).date_naive();
    let yesterday = today.pred_opt();
    let mut today_tokens = TokenCounts::default();
    let mut yesterday_tokens = TokenCounts::default();
    let mut hourly_tokens = [0_u64; 24];
    let mut daily_cost: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    let mut sessions: BTreeMap<(Tool, String), SessionOverview> = BTreeMap::new();
    let mut session_costs: BTreeMap<(Tool, String), f64> = BTreeMap::new();
    let recent_start = now - Duration::days(7);
    let current_five_hours: BTreeMap<_, _> = [Tool::Claude, Tool::Codex]
        .into_iter()
        .filter_map(|tool| current_five_hour_id(input.points, tool, now).map(|id| (tool, id)))
        .collect();
    for (index, event) in input.events.iter().enumerate() {
        let local = event.timestamp.with_timezone(&tz);
        let date = local.date_naive();
        if date == today {
            today_tokens.add_saturating(&event.tokens);
            let hour = local.hour() as usize;
            hourly_tokens[hour] =
                hourly_tokens[hour].saturating_add(event.tokens.total_without_reasoning());
        } else if Some(date) == yesterday {
            yesterday_tokens.add_saturating(&event.tokens);
        }
        if date.year() == today.year() && date.month() == today.month() {
            let cost = input
                .pricing
                .cost_usd(&event.model, &event.tokens)
                .unwrap_or(0.0);
            *daily_cost.entry(date).or_default() += cost;
        }
        if event.timestamp < recent_start || event.timestamp > now {
            continue;
        }
        let key = (event.tool, event.session_id.clone());
        *session_costs.entry(key.clone()).or_default() += input
            .pricing
            .cost_usd(&event.model, &event.tokens)
            .unwrap_or(0.0);
        let group = sessions.entry(key).or_insert_with(|| SessionOverview {
            tool: event.tool,
            session_id: event.session_id.clone(),
            title: input.titles.get(&event.session_id).cloned(),
            source: event.source,
            project: event.project.clone(),
            model: event.model.clone(),
            requests: 0,
            tokens: TokenCounts::default(),
            cache_hit_rate: None,
            five_hour_delta: 0.0,
            weekly_delta: 0.0,
            first_at: event.timestamp,
            last_at: event.timestamp,
        });
        group.requests += 1;
        group.tokens.add_saturating(&event.tokens);
        group.first_at = group.first_at.min(event.timestamp);
        group.last_at = group.last_at.max(event.timestamp);
        if let Some(delta) = input.attribution.events.get(index) {
            group.weekly_delta += delta.weekly_delta;
        }
    }
    for window in input.attribution.windows.iter().filter(|window| {
        window.kind == AttributionWindowKind::FiveHour
            && current_five_hours.get(&window.tool) == Some(&window.window_id)
    }) {
        for session in &window.by_session {
            if let Some(group) = sessions.get_mut(&(window.tool, session.key.clone())) {
                group.five_hour_delta += session.delta;
            }
        }
    }
    let peak_hour = hourly_tokens
        .iter()
        .enumerate()
        .max_by_key(|(hour, total)| (*total, std::cmp::Reverse(*hour)))
        .and_then(|(hour, total)| (*total > 0).then_some(hour as u32));
    let today_hit = cache_hit(&today_tokens);
    let yesterday_hit = cache_hit(&yesterday_tokens);
    if let Some(mut date) = NaiveDate::from_ymd_opt(today.year(), today.month(), 1) {
        while date <= today {
            daily_cost.entry(date).or_default();
            let Some(next) = date.succ_opt() else { break };
            date = next;
        }
    }
    let month_estimated_cost_usd = daily_cost.values().copied().sum();
    let daily_cost = daily_cost
        .into_iter()
        .map(|(date, estimated_cost_usd)| DayCost {
            date,
            estimated_cost_usd,
        })
        .collect();
    let mut recent_sessions: Vec<_> = sessions
        .into_values()
        .map(|mut s| {
            s.cache_hit_rate = cache_hit(&s.tokens);
            s
        })
        .collect();
    recent_sessions.sort_by(|a, b| {
        b.weekly_delta
            .total_cmp(&a.weekly_delta)
            .then_with(|| {
                session_costs
                    .get(&(b.tool, b.session_id.clone()))
                    .copied()
                    .unwrap_or(0.0)
                    .total_cmp(
                        &session_costs
                            .get(&(a.tool, a.session_id.clone()))
                            .copied()
                            .unwrap_or(0.0),
                    )
            })
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    recent_sessions.truncate(10);
    let all_recent_sessions: BTreeSet<_> = input
        .events
        .iter()
        .filter(|e| e.timestamp >= recent_start && e.timestamp <= now)
        .map(|e| (e.tool, e.session_id.as_str()))
        .collect();
    let requests = input
        .events
        .iter()
        .filter(|e| e.timestamp >= recent_start && e.timestamp <= now)
        .count();
    let mut high_prompt_keys: BTreeSet<_> = input
        .attribution
        .prompts
        .iter()
        .filter(|p| {
            p.weekly_delta >= 0.5
                && p.prompt_at
                    .is_some_and(|at| at >= recent_start && at <= now)
        })
        .filter_map(|p| Some((p.tool, p.session_id.clone(), p.prompt_at?)))
        .collect();
    for prompt in input.attribution.window_prompts.iter().filter(|p| {
        p.kind == AttributionWindowKind::FiveHour
            && current_five_hours.get(&p.tool) == Some(&p.window_id)
            && p.delta >= 1.0
            && p.prompt_at
                .is_some_and(|at| at >= recent_start && at <= now)
    }) {
        if let Some(at) = prompt.prompt_at {
            high_prompt_keys.insert((prompt.tool, prompt.session_id.clone(), at));
        }
    }
    let high_prompts = high_prompt_keys.len();
    let tools = [Tool::Claude, Tool::Codex]
        .into_iter()
        .map(|tool| {
            let plan = match tool {
                Tool::Claude => input
                    .claude_snapshots
                    .iter()
                    .max_by_key(|s| s.at)
                    .and_then(|s| s.subscription_type.clone()),
                Tool::Codex => input
                    .codex_snapshots
                    .iter()
                    .max_by_key(|s| s.at)
                    .and_then(|s| s.plan_type.clone())
                    .or_else(|| {
                        input
                            .codex_log
                            .iter()
                            .max_by_key(|s| s.timestamp)
                            .and_then(|s| s.plan_type.clone())
                    }),
            };
            ToolOverview {
                tool,
                plan,
                five_hour: display(input.points, tool, WindowKind::FiveHour, now),
                weekly: display(input.points, tool, WindowKind::Weekly, now),
            }
        })
        .collect();
    let trends = [Tool::Claude, Tool::Codex]
        .into_iter()
        .map(|tool| ToolTrend {
            tool,
            five_hour: current_five_hour(input.points, tool, now),
            high_prompts: prompt_markers(input, tool, now),
        })
        .collect();
    Overview {
        generated_at: now,
        tools,
        today_cache_read_share: (today_tokens.total_without_reasoning() > 0).then(|| {
            today_tokens.cache_read as f64 / today_tokens.total_without_reasoning() as f64
        }),
        today_tokens,
        peak_hour,
        hourly_tokens,
        today_cache_hit_rate: today_hit,
        cache_hit_change_from_yesterday: today_hit.zip(yesterday_hit).map(|(a, b)| a - b),
        month_estimated_cost_usd,
        daily_cost,
        recorder: input.recorder.clone(),
        trends,
        recent_sessions,
        summary: Summary {
            sessions: all_recent_sessions.len(),
            requests,
            high_prompts,
        },
    }
}
