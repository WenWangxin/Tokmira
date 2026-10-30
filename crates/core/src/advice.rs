//! Waste diagnosis ("省额度建议").
//!
//! Every rule works on usage fields and timestamps only; no conversation content is read.
//! Weekly quota percentages are never summed across tools because Claude and Codex have
//! separate quota pools, so the report is grouped per tool.
//!
//! Percent conversion: an event's extra share of the weekly quota is its attributed weekly
//! delta multiplied by (extra cost / event cost). When a tool has no attributed weekly
//! quota in the period, percentages are `None` and the UI falls back to cost and tokens.
//!
//! `event_tags` marks each request that hit a rule. Unlike `prompt_tags`, a request tag
//! does not require the diagnosis to explain 30% of the prompt's cost. One request keeps
//! a single tag: Error wins, then the earlier rule.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::attribution::EventAttribution;
use crate::environment::EnvironmentSnapshot;
use crate::forecast::{forecast, Forecast, WindowKind};
use crate::i18n::Lang;
use crate::model::{CompactEvent, PromptEvent, TokenCounts, Tool, UsageEvent};
use crate::pricing::{ModelPrices, Pricing};
use crate::quota_store::{normalize_claude_plan, ClaudeSnapshot, CodexSnapshot};
use crate::session_open::SessionRef;
use crate::timeline::QuotaPoint;

mod alerts;
mod next_step;
mod rules_m12;
pub use alerts::{AdviceAlert, AlertTracker};
pub use next_step::NextStep;

#[derive(Clone, Debug, Deserialize)]
pub struct SeverityConfig {
    pub error_weekly_pct: f64,
    pub error_cost_usd: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct CacheRebuildConfig {
    pub min_rebuild_tokens: u64,
    pub max_read_share: f64,
    pub claude_ttl_5m_minutes: i64,
    pub claude_ttl_1h_minutes: i64,
    pub codex_ttl_minutes: i64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LongContextConfig {
    pub threshold_tokens: u64,
    pub target_tokens: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LightTaskConfig {
    pub max_requests: u64,
    pub max_output_tokens: u64,
    pub claude_expensive: Vec<String>,
    pub codex_expensive: Vec<String>,
    pub claude_alternative: String,
    pub codex_alternative: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct SubagentConfig {
    pub min_cost_share: f64,
    pub min_requests: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ModelSwitchConfig {
    pub ignore_models: Vec<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ContextSpikeConfig {
    pub min_spike_tokens: u64,
    pub target_tokens: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RunawayPromptConfig {
    pub max_requests_per_prompt: u64,
    pub codex_max_requests_per_prompt: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ExpensiveSubagentConfig {
    pub min_requests: u64,
    pub claude_alternative: String,
    pub codex_alternative: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct HeavyReasoningConfig {
    pub min_reasoning_tokens: u64,
    pub min_reasoning_share: f64,
    pub keep_share: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct FastModeConfig {
    pub claude_price_multiplier: f64,
    pub claude_models: Vec<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct HighEffortLightTaskConfig {
    pub claude_high_efforts: Vec<String>,
    pub codex_high_efforts: Vec<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct FrequentCompactionConfig {
    pub min_compactions: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WebSearchConfig {
    pub min_searches: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct StartupConfig {
    pub bytes_per_token: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct QuotaRebalanceConfig {
    pub busy_weekly_pct: f64,
    pub idle_weekly_pct: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct PlanFitConfig {
    pub headroom: f64,
    pub min_weeks: usize,
    pub min_capped_weeks: usize,
    pub max_end_gap_hours: i64,
    pub claude_capacity: BTreeMap<String, f64>,
    pub chatgpt_capacity: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct AlertConfig {
    pub cooldown_minutes: i64,
    pub light_task_settle_minutes: i64,
    pub lookback_hours: i64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct QuotaHitConfig {
    /// The latest real reading, without any estimate on top, must reach this percent.
    pub threshold_pct: f64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AdviceLink {
    pub label: String,
    pub url: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct AdviceConfig {
    pub version: String,
    pub severity: SeverityConfig,
    pub cache_rebuild: CacheRebuildConfig,
    pub long_context: LongContextConfig,
    pub light_task: LightTaskConfig,
    pub subagents: SubagentConfig,
    pub model_switch: ModelSwitchConfig,
    pub context_spike: ContextSpikeConfig,
    pub runaway_prompt: RunawayPromptConfig,
    pub expensive_subagent: ExpensiveSubagentConfig,
    pub heavy_reasoning: HeavyReasoningConfig,
    pub fast_mode: FastModeConfig,
    pub high_effort_light_task: HighEffortLightTaskConfig,
    pub frequent_compaction: FrequentCompactionConfig,
    pub web_search: WebSearchConfig,
    pub session_startup: StartupConfig,
    pub quota_rebalance: QuotaRebalanceConfig,
    pub plan_fit: PlanFitConfig,
    pub alerts: AlertConfig,
    pub quota_hit: QuotaHitConfig,
    #[serde(default)]
    pub links: BTreeMap<String, AdviceLink>,
}

impl AdviceConfig {
    /// Parses the bundled thresholds.
    pub fn bundled() -> Result<Self, &'static str> {
        toml::from_str(include_str!("../../../config/advice.toml"))
            .map_err(|_| "内置建议配置格式错误")
    }

    /// Applies a user override file. Tables in the override replace whole sections;
    /// link entries are merged per rule.
    pub fn with_override(self, content: &str) -> Result<Self, &'static str> {
        let base: toml::Table = toml::from_str(include_str!("../../../config/advice.toml"))
            .map_err(|_| "内置建议配置格式错误")?;
        let patch: toml::Table = toml::from_str(content).map_err(|_| "覆盖建议配置格式错误")?;
        let mut merged = base;
        for (key, value) in patch {
            match (merged.get_mut(&key), value) {
                (Some(toml::Value::Table(existing)), toml::Value::Table(update)) => {
                    for (inner_key, inner_value) in update {
                        existing.insert(inner_key, inner_value);
                    }
                }
                (_, value) => {
                    merged.insert(key, value);
                }
            }
        }
        let _ = self;
        toml::Value::Table(merged)
            .try_into()
            .map_err(|_| "覆盖建议配置字段不正确")
    }

    fn link(&self, id: &str) -> Option<AdviceLink> {
        self.links
            .get(id)
            .filter(|link| !link.url.trim().is_empty() && !link.label.trim().is_empty())
            .cloned()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Serialize)]
pub struct Advice {
    /// Stable rule id, e.g. `cache_rebuild`.
    pub id: String,
    /// `<tool>:<id>`, the key stored in `AppSettings.dismissed_advice`.
    pub key: String,
    pub tool: Tool,
    pub title: String,
    pub severity: Severity,
    pub facts: Vec<String>,
    pub description: String,
    pub occurrences: u64,
    pub sessions: usize,
    pub overhead_tokens: u64,
    pub overhead_cost_usd: f64,
    pub consumed_weekly_pct: Option<f64>,
    /// False for unavoidable overhead (session start-up, subagents): reported, not counted as savings.
    pub avoidable: bool,
    pub avoidable_cost_usd: f64,
    pub avoidable_weekly_pct: Option<f64>,
    pub link: Option<AdviceLink>,
    /// One-line action; `command` is only set for commands verified on that tool.
    pub next_step: Option<NextStep>,
    /// The most recent session it happened in, where "fill command" types.
    pub latest_session: Option<SessionRef>,
    #[serde(skip)]
    session_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PromptTag {
    pub tool: Tool,
    pub session_id: String,
    pub prompt_at: DateTime<Utc>,
    pub tag: String,
    pub severity: Severity,
}

/// One request that hit at least one diagnosis rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EventTag {
    pub tool: Tool,
    pub session_id: String,
    /// Present when the log had a request id, so the timeline can tell apart
    /// requests that share a timestamp.
    pub request_id: Option<String>,
    pub at: DateTime<Utc>,
    pub tag: String,
    pub severity: Severity,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolAdviceSummary {
    pub avoidable_weekly_pct: Option<f64>,
    pub avoidable_cost_usd: f64,
    pub count: usize,
    pub sessions: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolAdvice {
    pub tool: Tool,
    pub has_quota_data: bool,
    pub summary: ToolAdviceSummary,
    pub items: Vec<Advice>,
    pub hidden: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdviceReport {
    pub generated_at: DateTime<Utc>,
    pub period_days: u32,
    pub config_version: String,
    pub tools: Vec<ToolAdvice>,
    pub prompt_tags: Vec<PromptTag>,
    /// One tag per request that hit a rule. The 30% prompt-cost gate does not apply.
    pub event_tags: Vec<EventTag>,
}

pub struct AdviceInput<'a> {
    pub events: &'a [EventAttribution],
    pub prompts: &'a [PromptEvent],
    pub pricing: &'a Pricing,
    pub config: &'a AdviceConfig,
    pub dismissed: &'a [String],
    pub compactions: &'a [CompactEvent],
    pub environment: Option<&'a EnvironmentSnapshot>,
    pub quotas: &'a [QuotaAdviceState],
}

#[derive(Clone, Debug, Default)]
pub struct QuotaAdviceState {
    pub tool: Option<Tool>,
    pub weekly_pct: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub predicted_hit_at: Option<DateTime<Utc>>,
    pub used_recently: bool,
    pub plan: Option<String>,
    pub completed_weekly_max: Vec<f64>,
}

pub fn quota_advice_states(
    points: &[QuotaPoint],
    claude: &[ClaudeSnapshot],
    codex: &[CodexSnapshot],
    events: &[UsageEvent],
    now: DateTime<Utc>,
    max_end_gap_hours: i64,
) -> Vec<QuotaAdviceState> {
    [Tool::Claude, Tool::Codex]
        .into_iter()
        .map(|tool| {
            let current = points
                .iter()
                .filter(|point| point.tool == tool && point.at <= now)
                .filter_map(|point| point.weekly.as_ref().map(|window| (point.at, window)))
                .filter(|(_, window)| window.resets_at.is_some_and(|reset| reset > now))
                .max_by_key(|(at, _)| *at)
                .map(|(_, window)| window);
            let mut weeks = BTreeMap::<i64, (f64, DateTime<Utc>, DateTime<Utc>)>::new();
            match tool {
                Tool::Claude => {
                    for snapshot in claude.iter().filter(|snapshot| snapshot.at <= now) {
                        if let Some(window) = &snapshot.seven_day {
                            if let Some(reset) = window.resets_at.filter(|reset| *reset <= now) {
                                if snapshot.at > reset {
                                    continue;
                                }
                                let entry = weeks
                                    .entry(reset.timestamp().div_euclid(60))
                                    .or_insert((0.0, snapshot.at, reset));
                                entry.0 = entry.0.max(window.used_percent);
                                if snapshot.at > entry.1 {
                                    entry.1 = snapshot.at;
                                    entry.2 = reset;
                                }
                            }
                        }
                    }
                }
                Tool::Codex => {
                    for snapshot in codex.iter().filter(|snapshot| {
                        snapshot.at <= now
                            && matches!(snapshot.limit_id.as_deref(), None | Some("codex"))
                    }) {
                        if let Some(window) = &snapshot.secondary {
                            if let Some(reset) = window.resets_at.filter(|reset| *reset <= now) {
                                if snapshot.at > reset {
                                    continue;
                                }
                                let entry = weeks
                                    .entry(reset.timestamp().div_euclid(60))
                                    .or_insert((0.0, snapshot.at, reset));
                                entry.0 = entry.0.max(window.used_percent);
                                if snapshot.at > entry.1 {
                                    entry.1 = snapshot.at;
                                    entry.2 = reset;
                                }
                            }
                        }
                    }
                }
            }
            let plan = match tool {
                Tool::Claude => claude
                    .iter()
                    .filter(|snapshot| {
                        snapshot.at <= now && snapshot.at >= now - Duration::days(30)
                    })
                    .max_by_key(|snapshot| snapshot.at)
                    .and_then(|snapshot| {
                        normalize_claude_plan(
                            snapshot.subscription_type.as_deref(),
                            snapshot.rate_limit_tier.as_deref(),
                        )
                    }),
                Tool::Codex => codex
                    .iter()
                    .filter(|snapshot| {
                        snapshot.at <= now
                            && snapshot.at >= now - Duration::days(30)
                            && matches!(snapshot.limit_id.as_deref(), None | Some("codex"))
                    })
                    .max_by_key(|snapshot| snapshot.at)
                    .and_then(|snapshot| snapshot.plan_type.clone()),
            };
            QuotaAdviceState {
                tool: Some(tool),
                weekly_pct: current.map(|window| window.used_percent),
                resets_at: current.and_then(|window| window.resets_at),
                predicted_hit_at: match forecast(points, tool, WindowKind::Weekly, now) {
                    Forecast::WillHit { at } => Some(at),
                    _ => None,
                },
                used_recently: events.iter().any(|event| {
                    event.tool == tool
                        && event.timestamp >= now - Duration::days(30)
                        && event.timestamp <= now
                }),
                plan,
                completed_weekly_max: weeks
                    .into_iter()
                    .rev()
                    .filter(|(_, (_, latest, reset))| {
                        *reset - *latest <= Duration::hours(max_end_gap_hours.max(0))
                    })
                    .map(|(_, (max, _, _))| max)
                    .collect(),
            }
        })
        .collect()
}

/// One event's contribution to a rule.
struct Hit<'a> {
    item: &'a EventAttribution,
    overhead_tokens: u64,
    overhead_cost: f64,
    avoidable_cost: f64,
    actual_cost: Option<f64>,
}

struct RuleOutput<'a> {
    id: &'static str,
    title: String,
    facts: Vec<String>,
    description: String,
    occurrences: u64,
    hits: Vec<Hit<'a>>,
    avoidable: bool,
    tag: &'static str,
}

fn write_tokens(tokens: &TokenCounts) -> u64 {
    tokens
        .cache_write_5m
        .saturating_add(tokens.cache_write_1h)
        .saturating_add(tokens.cache_write_unspecified)
}

fn context_tokens(tokens: &TokenCounts) -> u64 {
    tokens
        .input
        .saturating_add(tokens.cache_read)
        .saturating_add(write_tokens(tokens))
}

/// Tokens re-sent when the cache is cold: cache writes for Claude, uncached input for Codex.
fn rebuild_tokens(event: &UsageEvent) -> u64 {
    match event.tool {
        Tool::Claude => write_tokens(&event.tokens),
        Tool::Codex => event.tokens.input,
    }
}

fn rebuild_cost(event: &UsageEvent, prices: &ModelPrices) -> f64 {
    let tokens = &event.tokens;
    match event.tool {
        Tool::Claude => {
            (tokens.cache_write_5m as f64 * prices.cache_write_5m
                + tokens.cache_write_1h as f64 * prices.cache_write_1h
                + tokens.cache_write_unspecified as f64 * prices.cache_write_unspecified)
                / 1_000_000.0
        }
        Tool::Codex => tokens.input as f64 * prices.input / 1_000_000.0,
    }
}

/// Formats token counts the way the UI does: 万 / 亿 with one decimal.
pub fn format_tokens(value: u64) -> String {
    if value >= 100_000_000 {
        format!("{:.1} 亿", value as f64 / 100_000_000.0)
    } else if value >= 10_000 {
        format!("{:.1} 万", value as f64 / 10_000.0)
    } else {
        value.to_string()
    }
}

/// Token units for diagnostics and notifications in the selected language.
pub fn format_tokens_lang(value: u64, lang: Lang) -> String {
    if lang == Lang::Zh {
        return format_tokens(value);
    }
    for (threshold, suffix) in [(1_000_000_000, "B"), (1_000_000, "M"), (10_000, "K")] {
        if value >= threshold {
            let divisor = if suffix == "K" { 1_000 } else { threshold };
            return format!("{:.1}{suffix}", value as f64 / divisor as f64);
        }
    }
    value.to_string()
}

/// Linear-interpolated percentile of a sorted slice (`q` in 0..=1).
pub fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let position = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    Some(sorted[lower] + (sorted[upper] - sorted[lower]) * fraction)
}

#[derive(Clone, Debug, Serialize)]
pub struct PromptRequestDistribution {
    pub prompts: usize,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub maximum: usize,
}

/// Counts usage requests attributed to each prompt using timestamps only.
pub fn prompt_request_distribution(
    events: &[UsageEvent],
    prompts: &[PromptEvent],
    tool: Tool,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> PromptRequestDistribution {
    let mut starts: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
    for prompt in prompts.iter().filter(|prompt| {
        prompt.tool == tool && prompt.timestamp >= since && prompt.timestamp <= now
    }) {
        starts
            .entry(&prompt.session_id)
            .or_default()
            .push(prompt.timestamp);
    }
    let mut counts: BTreeMap<(&str, DateTime<Utc>), usize> = BTreeMap::new();
    for (session, times) in &mut starts {
        times.sort();
        times.dedup();
        for at in times.iter() {
            counts.insert((session, *at), 0);
        }
    }
    for event in events
        .iter()
        .filter(|event| event.tool == tool && event.timestamp >= since && event.timestamp <= now)
    {
        let Some(times) = starts.get(event.session_id.as_str()) else {
            continue;
        };
        if let Some(index) = times
            .partition_point(|at| *at <= event.timestamp)
            .checked_sub(1)
        {
            *counts
                .entry((event.session_id.as_str(), times[index]))
                .or_default() += 1;
        }
    }
    let mut values: Vec<f64> = counts.values().map(|count| *count as f64).collect();
    values.sort_by(f64::total_cmp);
    PromptRequestDistribution {
        prompts: values.len(),
        p50: percentile(&values, 0.5).unwrap_or(0.0),
        p90: percentile(&values, 0.9).unwrap_or(0.0),
        p95: percentile(&values, 0.95).unwrap_or(0.0),
        p99: percentile(&values, 0.99).unwrap_or(0.0),
        maximum: values.last().copied().unwrap_or(0.0) as usize,
    }
}

/// Candidate new writes in a warm, unchanged-model main session, before the size gate.
pub fn warm_middle_writes(
    events: &[UsageEvent],
    compactions: &[CompactEvent],
    tool: Tool,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    max_read_share: f64,
) -> Vec<u64> {
    let mut by_session: BTreeMap<&str, Vec<&UsageEvent>> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.tool == tool && !event.is_subagent && event.timestamp <= now)
    {
        by_session.entry(&event.session_id).or_default().push(event);
    }
    let mut writes = Vec::new();
    for requests in by_session.values_mut() {
        requests.sort_by_key(|event| event.timestamp);
        for pair in requests.windows(2) {
            let (previous, current) = (pair[0], pair[1]);
            if current.timestamp < since
                || previous.model != current.model
                || compactions.iter().any(|compact| {
                    compact.tool == tool
                        && compact.session_id == current.session_id
                        && compact.timestamp > previous.timestamp
                        && compact.timestamp <= current.timestamp
                })
            {
                continue;
            }
            let read = current.tokens.cache_read;
            let written = rebuild_tokens(current);
            if (read as f64) >= (1.0 - max_read_share) * (read + written) as f64 {
                writes.push(written);
            }
        }
    }
    writes
}

fn event_cost(pricing: &Pricing, event: &UsageEvent) -> f64 {
    pricing.cost_usd(&event.model, &event.tokens).unwrap_or(0.0)
}

fn sessions_in_order<'a>(
    events: &[&'a EventAttribution],
) -> BTreeMap<&'a str, Vec<&'a EventAttribution>> {
    let mut sessions: BTreeMap<&str, Vec<&EventAttribution>> = BTreeMap::new();
    for item in events {
        sessions
            .entry(item.event.session_id.as_str())
            .or_default()
            .push(item);
    }
    sessions
}

fn cache_rebuild<'a>(
    tool: Tool,
    main: &BTreeMap<&'a str, Vec<&'a EventAttribution>>,
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let config = &input.config.cache_rebuild;
    let mut hits = Vec::new();
    let mut gaps = Vec::new();
    let mut hour_ttl = 0_u64;
    for events in main.values() {
        for pair in events.windows(2) {
            let (previous, current) = (&pair[0].event, &pair[1].event);
            if current.timestamp < since {
                continue;
            }
            let uses_hour = previous.tokens.cache_write_1h > 0 || current.tokens.cache_write_1h > 0;
            let ttl = match tool {
                Tool::Claude if uses_hour => config.claude_ttl_1h_minutes,
                Tool::Claude => config.claude_ttl_5m_minutes,
                Tool::Codex => config.codex_ttl_minutes,
            };
            let gap = current.timestamp - previous.timestamp;
            let rebuilt = rebuild_tokens(current);
            let read = current.tokens.cache_read;
            if gap <= Duration::minutes(ttl)
                || rebuilt <= config.min_rebuild_tokens
                || read as f64 >= config.max_read_share * (read + rebuilt) as f64
            {
                continue;
            }
            if uses_hour {
                hour_ttl += 1;
            }
            gaps.push(gap.num_seconds() as f64 / 60.0);
            let extra = input
                .pricing
                .model_prices(&current.model)
                .map(|prices| {
                    (rebuild_cost(current, prices)
                        - rebuilt as f64 * prices.cache_read / 1_000_000.0)
                        .max(0.0)
                })
                .unwrap_or(0.0);
            hits.push(Hit {
                item: pair[1],
                overhead_tokens: rebuilt,
                overhead_cost: extra,
                avoidable_cost: extra,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    gaps.sort_by(f64::total_cmp);
    let low = percentile(&gaps, 0.25).unwrap_or(0.0).round();
    let high = percentile(&gaps, 0.75).unwrap_or(0.0).round();
    let total: u64 = hits.iter().map(|hit| hit.overhead_tokens).sum();
    let description = match tool {
        Tool::Claude if hour_ttl * 2 >= hits.len() as u64 => lang.pick("订阅版缓存的有效期为 1 小时。间隔超过 1 小时再继续对话时，之前的上下文需要整段重新写入缓存。建议在过期前继续，或开启新会话并只保留必要上下文。", "The recorded cache lifetime is 1 hour. Resuming after more than an hour rewrites the previous context. Continue before it expires, or start a new session with only the context you need."),
        Tool::Claude => lang.pick("缓存的有效期为 5 分钟。间隔超过 5 分钟再继续对话时，之前的上下文需要整段重新写入缓存。连续的工作尽量一次完成，长时间离开后可开启新会话并只保留必要上下文。", "The recorded cache lifetime is 5 minutes. Resuming after more than 5 minutes rewrites the previous context. Keep related work together, or start a new session with only the context you need after a long break."),
        Tool::Codex => lang.pick("Codex 未公开缓存时长。间隔较久再继续对话时，之前的上下文通常需要重新计入输入。长时间离开后可开启新会话并只保留必要上下文。", "Codex does not publish a cache lifetime. After a long break, previous context may count as uncached input again. Consider a new session with only the context you need."),
    };
    Some(RuleOutput {
        id: "cache_rebuild",
        title: lang
            .pick("缓存过期后的整段重建", "Context rebuilt after cache expiry")
            .to_owned(),
        facts: vec![
            crate::tr!(lang, "发生 {} 次", "{} occurrences", hits.len()),
            crate::tr!(
                lang,
                "累计重建 {} token",
                "{} tokens rebuilt in total",
                format_tokens_lang(total, lang)
            ),
            if low == high {
                crate::tr!(
                    lang,
                    "间隔约 {low:.0} 分钟",
                    "About {low:.0} minutes between requests"
                )
            } else {
                crate::tr!(
                    lang,
                    "间隔多为 {low:.0}–{high:.0} 分钟",
                    "Most gaps were {low:.0}–{high:.0} minutes"
                )
            },
        ],
        description: description.to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: lang.pick("缓存已过期", "Cache expired"),
    })
}

fn session_startup<'a>(
    main: &BTreeMap<&'a str, Vec<&'a EventAttribution>>,
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let mut hits = Vec::new();
    for events in main.values() {
        let Some(first) = events.first() else {
            continue;
        };
        if first.event.timestamp < since {
            continue;
        }
        let tokens = rebuild_tokens(&first.event);
        if tokens == 0 {
            continue;
        }
        let cost = input
            .pricing
            .model_prices(&first.event.model)
            .map(|prices| rebuild_cost(&first.event, prices))
            .unwrap_or(0.0);
        hits.push(Hit {
            item: first,
            overhead_tokens: tokens,
            overhead_cost: cost,
            avoidable_cost: 0.0,
            actual_cost: None,
        });
    }
    if hits.is_empty() {
        return None;
    }
    let average = hits.iter().map(|hit| hit.overhead_tokens).sum::<u64>() / hits.len() as u64;
    let mut starts: Vec<f64> = hits.iter().map(|hit| hit.overhead_tokens as f64).collect();
    starts.sort_by(f64::total_cmp);
    let baseline = percentile(&starts, 0.1).unwrap_or(0.0) as u64;
    let mut facts = vec![
        crate::tr!(lang, "{} 个新会话", "{} new sessions", hits.len()),
        crate::tr!(
            lang,
            "单次约 {} token",
            "About {} tokens per session",
            format_tokens_lang(average, lang)
        ),
        crate::tr!(
            lang,
            "比最低一批（p10）多约 {} token",
            "About {} tokens above the lowest group (p10)",
            format_tokens_lang(average.saturating_sub(baseline), lang)
        ),
    ];
    if let Some(environment) = input.environment {
        let tool = hits[0].item.event.tool;
        let (names, project_names, global_bytes, project_bytes) = match tool {
            Tool::Claude => (
                &environment.claude_global_mcp_names,
                Some(&environment.claude_project_mcp_names),
                environment.claude_instruction_bytes,
                &environment.claude_project_instruction_bytes,
            ),
            Tool::Codex => (
                &environment.codex_global_mcp_names,
                None,
                environment.codex_instruction_bytes,
                &environment.codex_project_instruction_bytes,
            ),
        };
        let shown = names
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(lang.pick("、", ", "));
        facts.push(crate::tr!(
            lang,
            "本机配置的全局 MCP 服务器 {} 个{}",
            "{} globally configured MCP servers{}",
            names.len(),
            if shown.is_empty() {
                String::new()
            } else {
                crate::tr!(lang, "：{shown}", ": {shown}")
            }
        ));
        if let Some(project_names) = project_names {
            let names: BTreeSet<&str> = hits
                .iter()
                .filter_map(|hit| hit.item.event.project.as_deref())
                .filter_map(|path| project_names.get(Path::new(path)))
                .flat_map(|names| names.iter().map(String::as_str))
                .collect();
            if !names.is_empty() {
                facts.push(crate::tr!(
                    lang,
                    "项目级 MCP 服务器 {} 个",
                    "{} project MCP servers",
                    names.len()
                ));
            }
        }
        if tool == Tool::Claude && environment.claude_plugins > 0 {
            facts.push(crate::tr!(
                lang,
                "启用 {} 个插件",
                "{} plugins enabled",
                environment.claude_plugins
            ));
        }
        let average_project_bytes = hits
            .iter()
            .map(|hit| {
                hit.item
                    .event
                    .project
                    .as_deref()
                    .and_then(|path| project_bytes.get(Path::new(path)))
                    .copied()
                    .unwrap_or(0)
            })
            .sum::<u64>()
            / hits.len() as u64;
        facts.push(crate::tr!(
            lang,
            "说明文件约 {} token",
            "About {} tokens in instruction files",
            format_tokens_lang(
                global_bytes.saturating_add(average_project_bytes)
                    / input.config.session_startup.bytes_per_token.max(1),
                lang
            )
        ));
    }
    Some(RuleOutput {
        id: "session_startup",
        title: lang
            .pick("新会话的启动开销", "New session startup overhead")
            .to_owned(),
        facts,
        description: if input.environment.is_some() {
            lang.pick("每个新会话都会加载配置、工具定义和说明文件。这里只统计本机配置文件；桌面端或账号同步的插件与工具不在其中。可检查不常用的 MCP 服务器和插件，连续的小任务可以在同一会话完成。", "Each new session loads configuration, tool definitions and instruction files. Only local configuration is counted here; desktop or account-synced tools and plugins are excluded. Review unused MCP servers and plugins, and keep related small tasks in one session.").to_owned()
        } else {
            lang.pick("每个新会话都需要先把系统提示和工具定义写入缓存。频繁新开会话会反复支付这部分开销；连续的小任务可以在同一会话里完成。", "Each new session first writes system instructions and tool definitions to the cache. Starting sessions repeatedly incurs this overhead again. Related small tasks can share a session.").to_owned()
        },
        occurrences: hits.len() as u64,
        hits,
        avoidable: false,
        tag: lang.pick("新会话启动开销", "Session startup"),
    })
}

fn long_context<'a>(
    main: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let config = &input.config.long_context;
    let mut hits = Vec::new();
    let mut longest = 0;
    let mut sum = 0_u64;
    for item in main {
        let context = context_tokens(&item.event.tokens);
        if context <= config.threshold_tokens {
            continue;
        }
        longest = longest.max(context);
        sum = sum.saturating_add(context);
        let excess = context
            .saturating_sub(config.target_tokens)
            .min(item.event.tokens.cache_read);
        let cost = input
            .pricing
            .model_prices(&item.event.model)
            .map(|prices| excess as f64 * prices.cache_read / 1_000_000.0)
            .unwrap_or(0.0);
        hits.push(Hit {
            item,
            overhead_tokens: excess,
            overhead_cost: cost,
            avoidable_cost: cost,
            actual_cost: None,
        });
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "long_context",
        title: lang.pick("上下文过长", "Long context").to_owned(),
        facts: vec![
            crate::tr!(lang, "{} 次请求", "{} requests", hits.len()),
            crate::tr!(lang, "平均上下文 {}", "Average context: {}", format_tokens_lang(sum / hits.len() as u64, lang)),
            crate::tr!(lang, "最长 {}", "Largest context: {}", format_tokens_lang(longest, lang)),
        ],
        description: lang.pick("上下文越长，每次请求都要重新读取越多缓存。任务阶段完成后可执行 /compact，或开启新会话。", "Longer context means more cached tokens are read on every request. After a task stage, use /compact or start a new session.").to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: lang.pick("上下文过长", "Long context"),
    })
}

fn model_family(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    if lower.contains("opus") {
        "Opus".to_owned()
    } else if lower.contains("fable") {
        "Fable".to_owned()
    } else if lower.contains("astra") {
        "GPT-6 Astra".to_owned()
    } else {
        model.to_owned()
    }
}

fn alternative_family(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    if lower.contains("sonnet") {
        "Sonnet".to_owned()
    } else {
        model.to_owned()
    }
}

fn expensive_light_task<'a>(
    tool: Tool,
    main: &[&'a EventAttribution],
    prompts: &[PromptEvent],
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let config = &input.config.light_task;
    let (patterns, alternative) = match tool {
        Tool::Claude => (&config.claude_expensive, &config.claude_alternative),
        Tool::Codex => (&config.codex_expensive, &config.codex_alternative),
    };
    let alternative_prices = input.pricing.model_prices(alternative)?;
    let mut prompt_times: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
    for prompt in prompts.iter().filter(|prompt| prompt.tool == tool) {
        prompt_times
            .entry(prompt.session_id.as_str())
            .or_default()
            .push(prompt.timestamp);
    }
    for times in prompt_times.values_mut() {
        times.sort();
        times.dedup();
    }
    let mut groups: BTreeMap<(&str, DateTime<Utc>), Vec<&EventAttribution>> = BTreeMap::new();
    for item in main {
        let Some(times) = prompt_times.get(item.event.session_id.as_str()) else {
            continue;
        };
        let position = times.partition_point(|time| *time <= item.event.timestamp);
        let Some(index) = position.checked_sub(1) else {
            continue;
        };
        if times[index] < since {
            continue;
        }
        groups
            .entry((item.event.session_id.as_str(), times[index]))
            .or_default()
            .push(item);
    }
    let mut hits = Vec::new();
    let mut prompts_hit = 0_u64;
    let mut output_sum = 0_u64;
    let mut cost_sum = 0.0;
    let mut families = BTreeMap::<String, u64>::new();
    for events in groups.values() {
        let expensive = events.iter().all(|item| {
            let model = item.event.model.to_ascii_lowercase();
            patterns
                .iter()
                .any(|pattern| model.contains(&pattern.to_ascii_lowercase()))
        });
        let output: u64 = events.iter().map(|item| item.event.tokens.output).sum();
        if !expensive
            || events.len() as u64 > config.max_requests
            || output >= config.max_output_tokens
        {
            continue;
        }
        prompts_hit += 1;
        output_sum += output;
        for item in events {
            let cost = event_cost(input.pricing, &item.event);
            let cheaper = alternative_prices.cost_usd(&item.event.tokens);
            cost_sum += cost;
            *families.entry(model_family(&item.event.model)).or_default() += 1;
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: (cost - cheaper).max(0.0),
                avoidable_cost: (cost - cheaper).max(0.0),
                actual_cost: None,
            });
        }
    }
    if prompts_hit == 0 {
        return None;
    }
    let family = families
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(family, _)| family)
        .unwrap_or_default();
    let saved: f64 = hits.iter().map(|hit| hit.avoidable_cost).sum();
    let share = if cost_sum > 0.0 {
        saved / cost_sum * 100.0
    } else {
        0.0
    };
    let alternative_name = alternative_family(alternative);
    let description = match tool {
        Tool::Claude => crate::tr!(lang, "重命名、补注释、运行测试等短小任务，换用 {alternative_name} 效果相近、额度消耗更低。Claude Code 中可执行 /model sonnet。", "For short tasks such as renaming, adding comments or running tests, consider {alternative_name} to reduce usage. In Claude Code, use /model sonnet and check that the result meets your needs."),
        Tool::Codex => crate::tr!(lang, "短小任务换用 {alternative_name} 效果相近、额度消耗更低。可在 Codex 中切换模型后再执行这类任务。", "Consider {alternative_name} for short tasks to reduce usage. Switch models in Codex before starting the task, and check that the result meets your needs."),
    };
    Some(RuleOutput {
        id: "expensive_light_task",
        title: crate::tr!(lang, "轻量任务使用了 {family}", "Light tasks used {family}"),
        facts: vec![
            crate::tr!(lang, "{prompts_hit} 次提示", "{prompts_hit} prompts"),
            crate::tr!(
                lang,
                "平均输出 {} token",
                "Average output: {} tokens",
                output_sum / prompts_hit
            ),
            crate::tr!(
                lang,
                "改用 {alternative_name} 约省 {share:.0}%",
                "About {share:.0}% lower API-equivalent cost with {alternative_name}"
            ),
        ],
        description,
        occurrences: prompts_hit,
        hits,
        avoidable: true,
        tag: lang.pick("轻量任务用了高价模型", "Expensive model for a light task"),
    })
}

fn many_subagents<'a>(
    period: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Option<RuleOutput<'a>> {
    let config = &input.config.subagents;
    let mut by_session: BTreeMap<&str, Vec<&EventAttribution>> = BTreeMap::new();
    for item in period {
        by_session
            .entry(item.event.session_id.as_str())
            .or_default()
            .push(item);
    }
    let mut hits = Vec::new();
    let mut sessions = 0_u64;
    let mut context_sum = 0_u64;
    for events in by_session.values() {
        let total: f64 = events
            .iter()
            .map(|item| event_cost(input.pricing, &item.event))
            .sum();
        let sub: Vec<_> = events
            .iter()
            .filter(|item| item.event.is_subagent)
            .collect();
        let sub_cost: f64 = sub
            .iter()
            .map(|item| event_cost(input.pricing, &item.event))
            .sum();
        if (sub.len() as u64) < config.min_requests
            || total <= 0.0
            || sub_cost / total < config.min_cost_share
        {
            continue;
        }
        sessions += 1;
        for item in sub {
            context_sum += context_tokens(&item.event.tokens);
            let cost = event_cost(input.pricing, &item.event);
            hits.push(Hit {
                item,
                overhead_tokens: item.event.tokens.total_without_reasoning(),
                overhead_cost: cost,
                avoidable_cost: 0.0,
                actual_cost: None,
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "many_subagents",
        title: lang.pick("子代理使用过多", "Heavy subagent usage").to_owned(),
        facts: vec![
            crate::tr!(lang, "{sessions} 个会话", "{sessions} sessions"),
            crate::tr!(lang, "子代理请求 {} 次", "{} subagent requests", hits.len()),
            crate::tr!(lang, "平均每次读入 {} token", "Average input per request: {} tokens",
                format_tokens_lang(context_sum / hits.len() as u64, lang)
            ),
        ],
        description:
            lang.pick("每个子代理都会重新读取一遍上下文。目标明确的小范围搜索，直接在主对话中完成更为经济。", "Each subagent reads its own context. A small, well-defined search may use less quota in the main session.")
                .to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: false,
        tag: lang.pick("子代理", "Subagents"),
    })
}

/// Share of the weekly quota represented by `part` of each hit's cost.
fn weekly_share(hits: &[Hit<'_>], pricing: &Pricing, part: impl Fn(&Hit<'_>) -> f64) -> f64 {
    hits.iter()
        .map(|hit| {
            let cost = hit
                .actual_cost
                .unwrap_or_else(|| event_cost(pricing, &hit.item.event));
            if cost <= 0.0 {
                0.0
            } else {
                hit.item.weekly_delta * (part(hit) / cost).clamp(0.0, 1.0)
            }
        })
        .sum()
}

fn finish(
    tool: Tool,
    rule: &RuleOutput<'_>,
    has_quota: bool,
    input: &AdviceInput<'_>,
    lang: Lang,
) -> Advice {
    let pricing = input.pricing;
    let overhead_cost: f64 = if rule.hits.is_empty() {
        0.0
    } else {
        rule.hits.iter().map(|hit| hit.overhead_cost).sum()
    };
    let avoidable_cost: f64 = if rule.hits.is_empty() {
        0.0
    } else {
        rule.hits.iter().map(|hit| hit.avoidable_cost).sum()
    };
    let attributed = rule.hits.iter().any(|hit| hit.item.weekly_delta != 0.0);
    let show_pct = has_quota && !rule.hits.is_empty() && (attributed || overhead_cost <= 0.0);
    let consumed = show_pct.then(|| weekly_share(&rule.hits, pricing, |hit| hit.overhead_cost));
    let avoidable_pct =
        show_pct.then(|| weekly_share(&rule.hits, pricing, |hit| hit.avoidable_cost));
    // Unavoidable overhead is informational, so only avoidable waste can be an Error.
    let severity_config = &input.config.severity;
    let severity = match (rule.avoidable, avoidable_pct) {
        (false, _) => Severity::Warning,
        (true, Some(pct)) if pct >= severity_config.error_weekly_pct => Severity::Error,
        (true, None) if avoidable_cost >= severity_config.error_cost_usd => Severity::Error,
        _ => Severity::Warning,
    };
    let session_ids: BTreeSet<String> = rule
        .hits
        .iter()
        .map(|hit| hit.item.event.session_id.clone())
        .collect();
    Advice {
        id: rule.id.to_owned(),
        key: format!("{}:{}", tool_key(tool), rule.id),
        tool,
        title: rule.title.clone(),
        severity,
        facts: rule.facts.clone(),
        description: rule.description.clone(),
        occurrences: rule.occurrences,
        sessions: session_ids.len(),
        overhead_tokens: rule.hits.iter().map(|hit| hit.overhead_tokens).sum(),
        overhead_cost_usd: overhead_cost,
        consumed_weekly_pct: consumed,
        avoidable: rule.avoidable,
        avoidable_cost_usd: avoidable_cost,
        avoidable_weekly_pct: if rule.avoidable {
            avoidable_pct
        } else {
            show_pct.then_some(0.0)
        },
        link: input.config.link(rule.id),
        next_step: next_step::next_step(tool, rule.id, lang, input.config),
        latest_session: rule
            .hits
            .iter()
            .max_by_key(|hit| hit.item.event.timestamp)
            .map(|hit| SessionRef::of(&hit.item.event)),
        session_ids,
    }
}

pub fn tool_key(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => "claude",
        Tool::Codex => "codex",
    }
}

/// Minimum share of a prompt's cost a diagnosis must explain before the prompt is tagged,
/// so a small start-up overhead does not label a prompt whose cost came from long work.
const TAG_MIN_COST_SHARE: f64 = 0.3;

fn prompt_key(
    prompt_times: &BTreeMap<&str, Vec<DateTime<Utc>>>,
    event: &UsageEvent,
) -> Option<DateTime<Utc>> {
    let times = prompt_times.get(event.session_id.as_str())?;
    let position = times.partition_point(|time| *time <= event.timestamp);
    position.checked_sub(1).map(|index| times[index])
}

/// One chosen request tag, plus the scan index used to order tags that share a timestamp.
struct ChosenTag {
    index: usize,
    tag: EventTag,
}

/// Keeps one tag per event. Error replaces Warning; an earlier rule keeps its tag
/// when the new hit has the same severity.
fn record_event_tag(
    chosen: &mut HashMap<*const EventAttribution, ChosenTag>,
    positions: &HashMap<*const EventAttribution, usize>,
    rule: &RuleOutput<'_>,
    severity: Severity,
) {
    for hit in &rule.hits {
        let key = std::ptr::from_ref(hit.item);
        let replace = match chosen.get(&key) {
            Some(existing) if existing.tag.severity == Severity::Error => false,
            Some(_) if severity != Severity::Error => false,
            _ => true,
        };
        if replace {
            chosen.insert(
                key,
                ChosenTag {
                    index: positions.get(&key).copied().unwrap_or(usize::MAX),
                    tag: EventTag {
                        tool: hit.item.event.tool,
                        session_id: hit.item.event.session_id.clone(),
                        request_id: hit.item.event.request_id.clone(),
                        at: hit.item.event.timestamp,
                        tag: rule.tag.to_owned(),
                        severity,
                    },
                },
            );
        }
    }
}

fn prompt_tags_for(
    tool: Tool,
    rule: &RuleOutput<'_>,
    severity: Severity,
    prompt_times: &BTreeMap<&str, Vec<DateTime<Utc>>>,
    prompt_costs: &BTreeMap<(String, DateTime<Utc>), f64>,
    tags: &mut BTreeMap<(Tool, String, DateTime<Utc>), (String, Severity)>,
) {
    let mut overhead: BTreeMap<(String, DateTime<Utc>), f64> = BTreeMap::new();
    for hit in &rule.hits {
        let event = &hit.item.event;
        if let Some(prompt_at) = prompt_key(prompt_times, event) {
            *overhead
                .entry((event.session_id.clone(), prompt_at))
                .or_default() += hit.overhead_cost;
        }
    }
    for ((session_id, prompt_at), cost) in overhead {
        let total = prompt_costs
            .get(&(session_id.clone(), prompt_at))
            .copied()
            .unwrap_or(0.0);
        if total <= 0.0 || cost / total < TAG_MIN_COST_SHARE {
            continue;
        }
        let key = (tool, session_id, prompt_at);
        // Keep one tag per prompt, preferring Error over Warning.
        match tags.get(&key) {
            Some((_, Severity::Error)) => {}
            Some(_) if severity == Severity::Warning => {}
            _ => {
                tags.insert(key, (rule.tag.to_owned(), severity));
            }
        }
    }
}

/// Builds the advice report for the last `period_days` days.
/// A tool's requests in the period and the rules they hit.
struct ToolRules<'a> {
    period: Vec<&'a EventAttribution>,
    rules: Vec<RuleOutput<'a>>,
}

fn tool_rules<'a>(
    tool: Tool,
    input: &AdviceInput<'a>,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
    lang: Lang,
) -> Option<ToolRules<'a>> {
    let mut all: Vec<&EventAttribution> = input
        .events
        .iter()
        .filter(|item| item.event.tool == tool)
        .collect();
    all.sort_by_key(|item| item.event.timestamp);
    let period: Vec<&EventAttribution> = all
        .iter()
        .copied()
        .filter(|item| item.event.timestamp >= since && item.event.timestamp <= now)
        .collect();
    let quota_rules = rules_m12::quota_rules(tool, input, lang);
    if period.is_empty() && quota_rules.is_empty() {
        return None;
    }
    // Session order uses all history so "first request" means the true first request.
    let main_all: Vec<&EventAttribution> = all
        .iter()
        .copied()
        .filter(|item| !item.event.is_subagent)
        .collect();
    let main_period: Vec<&EventAttribution> = period
        .iter()
        .copied()
        .filter(|item| !item.event.is_subagent)
        .collect();
    let sessions = sessions_in_order(&main_all);
    let mut rules: Vec<RuleOutput<'a>> = [
        cache_rebuild(tool, &sessions, since, input, lang),
        session_startup(&sessions, since, input, lang),
        long_context(&main_period, input, lang),
        expensive_light_task(tool, &main_period, input.prompts, since, input, lang),
        many_subagents(&period, input, lang),
    ]
    .into_iter()
    .flatten()
    .collect();
    rules.extend(rules_m12::usage_rules(
        tool,
        &sessions,
        &period,
        input.prompts,
        since,
        now,
        input,
        lang,
    ));
    rules.extend(quota_rules);
    Some(ToolRules { period, rules })
}

pub fn build_advice(input: &AdviceInput<'_>, period_days: u32, now: DateTime<Utc>) -> AdviceReport {
    build_advice_lang(input, period_days, now, Lang::Zh)
}

/// Localized presentation of the same diagnosis; amounts, keys and rule decisions are unchanged.
pub fn build_advice_lang(
    input: &AdviceInput<'_>,
    period_days: u32,
    now: DateTime<Utc>,
    lang: Lang,
) -> AdviceReport {
    let since = now - Duration::days(i64::from(period_days));
    let mut tools = Vec::new();
    let mut tags = BTreeMap::new();
    let mut chosen_tags_all = Vec::new();
    let positions: HashMap<*const EventAttribution, usize> = input
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| (std::ptr::from_ref(event), index))
        .collect();
    for tool in [Tool::Claude, Tool::Codex] {
        let Some(ToolRules { period, rules }) = tool_rules(tool, input, since, now, lang) else {
            continue;
        };
        let has_quota = period.iter().any(|item| item.weekly_delta > 0.0);
        let mut prompt_times: BTreeMap<&str, Vec<DateTime<Utc>>> = BTreeMap::new();
        for prompt in input.prompts.iter().filter(|prompt| prompt.tool == tool) {
            prompt_times
                .entry(prompt.session_id.as_str())
                .or_default()
                .push(prompt.timestamp);
        }
        for times in prompt_times.values_mut() {
            times.sort();
            times.dedup();
        }
        let mut prompt_costs: BTreeMap<(String, DateTime<Utc>), f64> = BTreeMap::new();
        for item in &period {
            if let Some(prompt_at) = prompt_key(&prompt_times, &item.event) {
                *prompt_costs
                    .entry((item.event.session_id.clone(), prompt_at))
                    .or_default() += event_cost(input.pricing, &item.event);
            }
        }
        let mut chosen_tags = HashMap::new();
        let mut items = Vec::new();
        let mut hidden = 0;
        for rule in rules {
            let advice = finish(tool, &rule, has_quota, input, lang);
            record_event_tag(&mut chosen_tags, &positions, &rule, advice.severity);
            prompt_tags_for(
                tool,
                &rule,
                advice.severity,
                &prompt_times,
                &prompt_costs,
                &mut tags,
            );
            if input.dismissed.iter().any(|key| key == &advice.key) {
                hidden += 1;
            } else {
                items.push(advice);
            }
        }
        items.sort_by(|a, b| {
            let key = |advice: &Advice| {
                (
                    advice.avoidable_weekly_pct.unwrap_or(-1.0),
                    advice.avoidable_cost_usd,
                    advice.overhead_cost_usd,
                )
            };
            let (a, b) = (key(a), key(b));
            b.0.total_cmp(&a.0)
                .then(b.1.total_cmp(&a.1))
                .then(b.2.total_cmp(&a.2))
        });
        let sessions_touched: BTreeSet<&String> = items
            .iter()
            .flat_map(|advice| advice.session_ids.iter())
            .collect();
        let attributed_savings: Vec<f64> = items
            .iter()
            .filter_map(|advice| advice.avoidable_weekly_pct)
            .collect();
        let summary = ToolAdviceSummary {
            avoidable_weekly_pct: (has_quota && !attributed_savings.is_empty())
                .then(|| attributed_savings.into_iter().sum()),
            avoidable_cost_usd: items
                .iter()
                .fold(0.0, |sum, advice| sum + advice.avoidable_cost_usd),
            count: items.len(),
            sessions: sessions_touched.len(),
        };
        chosen_tags_all.extend(chosen_tags.into_values());
        tools.push(ToolAdvice {
            tool,
            has_quota_data: has_quota,
            summary,
            items,
            hidden,
        });
    }
    AdviceReport {
        generated_at: now,
        period_days,
        config_version: input.config.version.clone(),
        tools,
        prompt_tags: tags
            .into_iter()
            .map(
                |((tool, session_id, prompt_at), (tag, severity))| PromptTag {
                    tool,
                    session_id,
                    prompt_at,
                    tag,
                    severity,
                },
            )
            .collect(),
        // Same-timestamp tags stay in request order so the session timeline can
        // attach each one to the matching request.
        event_tags: {
            chosen_tags_all.sort_by(|left, right| {
                left.tag
                    .tool
                    .cmp(&right.tag.tool)
                    .then(left.tag.session_id.cmp(&right.tag.session_id))
                    .then(left.tag.at.cmp(&right.tag.at))
                    .then(left.tag.request_id.cmp(&right.tag.request_id))
                    .then(left.index.cmp(&right.index))
            });
            chosen_tags_all.into_iter().map(|item| item.tag).collect()
        },
    }
}
