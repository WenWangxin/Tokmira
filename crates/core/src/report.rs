use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::model::{Source, TokenCounts, Tool, UsageEvent};
use crate::pricing::Pricing;

pub struct ReportOptions<Tz: TimeZone> {
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub tool: Option<Tool>,
    pub tz: Tz,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    pub events: u64,
    pub tokens: TokenCounts,
    pub cache_hit_rate: Option<f64>,
    /// Known-price subtotal; check unpriced_events before treating it as complete.
    pub estimated_cost_usd: f64,
    pub unpriced_events: u64,
    pub unpriced_tokens: TokenCounts,
}

impl Metrics {
    fn add(&mut self, event: &UsageEvent, cost: Option<f64>) {
        self.events += 1;
        self.tokens.add_saturating(&event.tokens);
        match cost {
            Some(cost) => self.estimated_cost_usd += cost,
            None => {
                self.unpriced_events += 1;
                self.unpriced_tokens.add_saturating(&event.tokens);
            }
        }
    }

    fn finish(&mut self) {
        let denominator = self.tokens.input as u128
            + self.tokens.cache_read as u128
            + self.tokens.cache_write_5m as u128
            + self.tokens.cache_write_1h as u128
            + self.tokens.cache_write_unspecified as u128;
        self.cache_hit_rate =
            (denominator != 0).then(|| self.tokens.cache_read as f64 / denominator as f64);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Group<K> {
    pub key: K,
    #[serde(flatten)]
    pub metrics: Metrics,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionGroup {
    pub key: String,
    pub tool: Tool,
    pub source: Source,
    pub project: Option<String>,
    pub models: Vec<String>,
    pub first_timestamp: DateTime<Utc>,
    pub last_timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub metrics: Metrics,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub pricing_version: String,
    pub pricing_note: String,
    pub total: Metrics,
    pub by_day: Vec<Group<NaiveDate>>,
    pub by_tool: Vec<Group<Tool>>,
    pub by_source: Vec<Group<Source>>,
    pub by_model: Vec<Group<String>>,
    pub by_project: Vec<Group<Option<String>>>,
    pub by_session: Vec<SessionGroup>,
    /// `false` is the main conversation; `true` is a subagent.
    pub by_subagent: Vec<Group<bool>>,
}

fn add_group<K: Ord + Clone>(
    groups: &mut BTreeMap<K, Metrics>,
    key: K,
    event: &UsageEvent,
    cost: Option<f64>,
) {
    groups.entry(key).or_default().add(event, cost);
}

fn into_groups<K: Ord>(groups: BTreeMap<K, Metrics>, by_day: bool) -> Vec<Group<K>> {
    let mut result: Vec<_> = groups
        .into_iter()
        .map(|(key, mut metrics)| {
            metrics.finish();
            Group { key, metrics }
        })
        .collect();
    if !by_day {
        result.sort_by(|a, b| {
            b.metrics
                .estimated_cost_usd
                .total_cmp(&a.metrics.estimated_cost_usd)
                .then_with(|| a.key.cmp(&b.key))
        });
    }
    result
}

struct SessionAccumulator {
    source: Source,
    project: Option<String>,
    models: BTreeSet<String>,
    first_timestamp: DateTime<Utc>,
    last_timestamp: DateTime<Utc>,
    metrics: Metrics,
}

/// Builds all views in a stable timestamp order, preserving input order for ties.
#[must_use]
pub fn build_report<Tz: TimeZone>(
    events: &[UsageEvent],
    pricing: &Pricing,
    options: &ReportOptions<Tz>,
) -> Report {
    let mut ordered: Vec<_> = events.iter().collect();
    ordered.sort_by_key(|event| event.timestamp);

    let mut total = Metrics::default();
    let mut days = BTreeMap::new();
    let mut tools = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut sessions: BTreeMap<(Tool, String), SessionAccumulator> = BTreeMap::new();
    let mut subagents = BTreeMap::new();

    for event in ordered {
        let day = event.timestamp.with_timezone(&options.tz).date_naive();
        if options.since.is_some_and(|since| day < since)
            || options.until.is_some_and(|until| day > until)
            || options.tool.is_some_and(|tool| event.tool != tool)
        {
            continue;
        }
        let cost = pricing.cost_usd(&event.model, &event.tokens);
        total.add(event, cost);
        add_group(&mut days, day, event, cost);
        add_group(&mut tools, event.tool, event, cost);
        add_group(&mut sources, event.source, event, cost);
        add_group(&mut models, event.model.clone(), event, cost);
        add_group(&mut projects, event.project.clone(), event, cost);
        add_group(&mut subagents, event.is_subagent, event, cost);

        let session = sessions
            .entry((event.tool, event.session_id.clone()))
            .or_insert_with(|| SessionAccumulator {
                source: event.source,
                project: event.project.clone(),
                models: BTreeSet::new(),
                first_timestamp: event.timestamp,
                last_timestamp: event.timestamp,
                metrics: Metrics::default(),
            });
        session.models.insert(event.model.clone());
        session.last_timestamp = event.timestamp;
        session.metrics.add(event, cost);
    }
    total.finish();

    let mut by_session: Vec<_> = sessions
        .into_iter()
        .map(|((tool, key), mut group)| {
            group.metrics.finish();
            SessionGroup {
                key,
                tool,
                source: group.source,
                project: group.project,
                models: group.models.into_iter().collect(),
                first_timestamp: group.first_timestamp,
                last_timestamp: group.last_timestamp,
                metrics: group.metrics,
            }
        })
        .collect();
    by_session.sort_by(|a, b| {
        b.metrics
            .estimated_cost_usd
            .total_cmp(&a.metrics.estimated_cost_usd)
            .then_with(|| (a.tool, &a.key).cmp(&(b.tool, &b.key)))
    });

    Report {
        pricing_version: pricing.version.clone(),
        pricing_note: pricing.note.clone(),
        total,
        by_day: into_groups(days, true),
        by_tool: into_groups(tools, false),
        by_source: into_groups(sources, false),
        by_model: into_groups(models, false),
        by_project: into_groups(projects, false),
        by_session,
        by_subagent: into_groups(subagents, false),
    }
}
