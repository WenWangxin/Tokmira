//! Waste diagnosis ("省额度建议").
//!
//! Every rule works on usage fields and timestamps only; no conversation content is read.
//! Weekly quota percentages are never summed across tools because Claude and Codex have
//! separate quota pools, so the report is grouped per tool.
//!
//! Percent conversion: an event's extra share of the weekly quota is its attributed weekly
//! delta multiplied by (extra cost / event cost). When a tool has no attributed weekly
//! quota in the period, percentages are `None` and the UI falls back to cost and tokens.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::attribution::EventAttribution;
use crate::model::{PromptEvent, TokenCounts, Tool, UsageEvent};
use crate::pricing::{ModelPrices, Pricing};

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
}

pub struct AdviceInput<'a> {
    pub events: &'a [EventAttribution],
    pub prompts: &'a [PromptEvent],
    pub pricing: &'a Pricing,
    pub config: &'a AdviceConfig,
    pub dismissed: &'a [String],
}

/// One event's contribution to a rule.
struct Hit<'a> {
    item: &'a EventAttribution,
    overhead_tokens: u64,
    overhead_cost: f64,
    avoidable_cost: f64,
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
        Tool::Claude if hour_ttl * 2 >= hits.len() as u64 => "订阅版缓存的有效期为 1 小时。间隔超过 1 小时再继续对话时，之前的上下文需要整段重新写入缓存。建议在过期前继续，或开启新会话并只保留必要上下文。",
        Tool::Claude => "缓存的有效期为 5 分钟。间隔超过 5 分钟再继续对话时，之前的上下文需要整段重新写入缓存。连续的工作尽量一次完成，长时间离开后可开启新会话并只保留必要上下文。",
        Tool::Codex => "Codex 未公开缓存时长。间隔较久再继续对话时，之前的上下文通常需要重新计入输入。长时间离开后可开启新会话并只保留必要上下文。",
    };
    Some(RuleOutput {
        id: "cache_rebuild",
        title: "缓存过期后的整段重建".to_owned(),
        facts: vec![
            format!("发生 {} 次", hits.len()),
            format!("累计重建 {} token", format_tokens(total)),
            if low == high {
                format!("间隔约 {low:.0} 分钟")
            } else {
                format!("间隔多为 {low:.0}–{high:.0} 分钟")
            },
        ],
        description: description.to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: "缓存已过期",
    })
}

fn session_startup<'a>(
    main: &BTreeMap<&'a str, Vec<&'a EventAttribution>>,
    since: DateTime<Utc>,
    input: &AdviceInput<'_>,
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
        });
    }
    if hits.is_empty() {
        return None;
    }
    let average = hits.iter().map(|hit| hit.overhead_tokens).sum::<u64>() / hits.len() as u64;
    Some(RuleOutput {
        id: "session_startup",
        title: "新会话的启动开销".to_owned(),
        facts: vec![
            format!("{} 个新会话", hits.len()),
            format!("单次约 {} token", format_tokens(average)),
        ],
        description: "每个新会话都需要先把系统提示和工具定义写入缓存。频繁新开会话会反复支付这部分开销；连续的小任务可以在同一会话里完成。".to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: false,
        tag: "新会话启动开销",
    })
}

fn long_context<'a>(
    main: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
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
        });
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "long_context",
        title: "上下文过长".to_owned(),
        facts: vec![
            format!("{} 次请求", hits.len()),
            format!("平均上下文 {}", format_tokens(sum / hits.len() as u64)),
            format!("最长 {}", format_tokens(longest)),
        ],
        description: "上下文越长，每次请求都要重新读取越多缓存。任务阶段完成后可执行 /compact，或开启新会话。".to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: true,
        tag: "上下文过长",
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
        Tool::Claude => format!("重命名、补注释、运行测试等短小任务，换用 {alternative_name} 效果相近、额度消耗更低。Claude Code 中可执行 /model sonnet。"),
        Tool::Codex => format!("短小任务换用 {alternative_name} 效果相近、额度消耗更低。可在 Codex 中切换模型后再执行这类任务。"),
    };
    Some(RuleOutput {
        id: "expensive_light_task",
        title: format!("轻量任务使用了 {family}"),
        facts: vec![
            format!("{prompts_hit} 次提示"),
            format!("平均输出 {} token", output_sum / prompts_hit),
            format!("改用 {alternative_name} 约省 {share:.0}%"),
        ],
        description,
        occurrences: prompts_hit,
        hits,
        avoidable: true,
        tag: "轻量任务用了高价模型",
    })
}

fn many_subagents<'a>(
    period: &[&'a EventAttribution],
    input: &AdviceInput<'_>,
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
            });
        }
    }
    if hits.is_empty() {
        return None;
    }
    Some(RuleOutput {
        id: "many_subagents",
        title: "子代理使用过多".to_owned(),
        facts: vec![
            format!("{sessions} 个会话"),
            format!("子代理请求 {} 次", hits.len()),
            format!(
                "平均每次读入 {} token",
                format_tokens(context_sum / hits.len() as u64)
            ),
        ],
        description:
            "每个子代理都会重新读取一遍上下文。目标明确的小范围搜索，直接在主对话中完成更为经济。"
                .to_owned(),
        occurrences: hits.len() as u64,
        hits,
        avoidable: false,
        tag: "子代理",
    })
}

/// Share of the weekly quota represented by `part` of each hit's cost.
fn weekly_share(hits: &[Hit<'_>], pricing: &Pricing, part: impl Fn(&Hit<'_>) -> f64) -> f64 {
    hits.iter()
        .map(|hit| {
            let cost = event_cost(pricing, &hit.item.event);
            if cost <= 0.0 {
                0.0
            } else {
                hit.item.weekly_delta * (part(hit) / cost).clamp(0.0, 1.0)
            }
        })
        .sum()
}

fn finish(tool: Tool, rule: &RuleOutput<'_>, has_quota: bool, input: &AdviceInput<'_>) -> Advice {
    let pricing = input.pricing;
    let overhead_cost: f64 = rule.hits.iter().map(|hit| hit.overhead_cost).sum();
    let avoidable_cost: f64 = rule.hits.iter().map(|hit| hit.avoidable_cost).sum();
    let consumed = has_quota.then(|| weekly_share(&rule.hits, pricing, |hit| hit.overhead_cost));
    let avoidable_pct =
        has_quota.then(|| weekly_share(&rule.hits, pricing, |hit| hit.avoidable_cost));
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
            has_quota.then_some(0.0)
        },
        link: input.config.link(rule.id),
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
pub fn build_advice(input: &AdviceInput<'_>, period_days: u32, now: DateTime<Utc>) -> AdviceReport {
    let since = now - Duration::days(i64::from(period_days));
    let mut tools = Vec::new();
    let mut tags = BTreeMap::new();
    for tool in [Tool::Claude, Tool::Codex] {
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
        if period.is_empty() {
            continue;
        }
        let has_quota = period.iter().any(|item| item.weekly_delta > 0.0);
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
        let rules: Vec<RuleOutput<'_>> = [
            cache_rebuild(tool, &sessions, since, input),
            session_startup(&sessions, since, input),
            long_context(&main_period, input),
            expensive_light_task(tool, &main_period, input.prompts, since, input),
            many_subagents(&period, input),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut items = Vec::new();
        let mut hidden = 0;
        for rule in rules {
            let advice = finish(tool, &rule, has_quota, input);
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
        let summary = ToolAdviceSummary {
            avoidable_weekly_pct: has_quota.then(|| {
                items
                    .iter()
                    .filter_map(|advice| advice.avoidable_weekly_pct)
                    .sum()
            }),
            avoidable_cost_usd: items.iter().map(|advice| advice.avoidable_cost_usd).sum(),
            count: items.len(),
            sessions: sessions_touched.len(),
        };
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
    }
}
