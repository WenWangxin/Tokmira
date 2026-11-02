//! Yuan costs and the dashboard, usage and advice views for DeepSeek Harness.
//!
//! DeepSeek bills a prepaid wallet in yuan, with no quota windows, so these views
//! stand apart from the dollar and percentage figures used for Claude and Codex.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::deepseek::{is_official_route, DeepSeekScan, DeepSeekTitleCall};
use crate::i18n::Lang;
use crate::model::{TokenCounts, Tool, UsageEvent};
use crate::pricing::DeepSeekPricing;
use crate::usage::{directory_name, session_display_name};

/// Below this many days of balance left at the current pace, the card warns.
pub const LOW_BALANCE_DAYS: f64 = 3.0;
/// How many days of spending set the pace for "days left".
pub const PACE_DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BalanceOrigin {
    Official,
    Manual,
}

/// A wallet reading in yuan. Manual readings only know the total.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeepSeekBalance {
    pub total: f64,
    pub topped_up: Option<f64>,
    pub granted: Option<f64>,
    pub origin: BalanceOrigin,
    pub as_of: DateTime<Utc>,
    /// For a manual reading: the estimated spend already taken off since it was entered.
    #[serde(default)]
    pub spent_since: Option<f64>,
}

/// A balance the user typed in, kept in settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManualBalance {
    pub amount: f64,
    pub recorded_at: DateTime<Utc>,
}

pub struct DeepSeekCosting<'a> {
    scan: &'a DeepSeekScan,
    pricing: &'a DeepSeekPricing,
    by_session: BTreeMap<&'a str, Vec<&'a UsageEvent>>,
}

impl<'a> DeepSeekCosting<'a> {
    #[must_use]
    pub fn new(scan: &'a DeepSeekScan, pricing: &'a DeepSeekPricing) -> Self {
        let mut by_session: BTreeMap<&str, Vec<&UsageEvent>> = BTreeMap::new();
        for event in &scan.events {
            by_session
                .entry(event.session_id.as_str())
                .or_default()
                .push(event);
        }
        Self {
            scan,
            pricing,
            by_session,
        }
    }

    /// None for other tools, custom routes and models without an official price.
    #[must_use]
    pub fn event_cost(&self, event: &UsageEvent) -> Option<f64> {
        if event.tool != Tool::DeepSeek || self.scan.is_custom_route(event) {
            return None;
        }
        self.pricing
            .cost_cny(&event.model, &event.tokens, event.timestamp)
    }

    /// Estimated from the request size; the log records no usage for it.
    #[must_use]
    pub fn title_cost(&self, call: &DeepSeekTitleCall) -> Option<f64> {
        if !is_official_route(call.provider.as_deref()) {
            return None;
        }
        self.pricing.cost_cny(
            &call.model,
            &self.pricing.title_tokens(call.bytes),
            call.timestamp,
        )
    }

    /// Yuan for one session's requests in `[from, to]`; None when none of them is priced.
    #[must_use]
    pub fn session_cost(
        &self,
        session_id: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Option<f64> {
        self.by_session
            .get(session_id)?
            .iter()
            .filter(|event| event.timestamp >= from && event.timestamp <= to)
            .filter_map(|event| self.event_cost(event))
            .fold(None, |sum, cost| Some(sum.unwrap_or(0.0) + cost))
    }

    /// Requests and title calls in `[from, to]`, in yuan.
    /// Folds from +0.0: a float `sum()` of nothing is -0.0, which prints as "¥-0.00".
    fn spend(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
        let requests: f64 = self
            .scan
            .events
            .iter()
            .filter(|event| event.timestamp >= from && event.timestamp <= to)
            .filter_map(|event| self.event_cost(event))
            .fold(0.0, |sum, cost| sum + cost);
        let titles: f64 = self
            .scan
            .title_calls
            .iter()
            .filter(|call| call.timestamp >= from && call.timestamp <= to)
            .filter_map(|call| self.title_cost(call))
            .fold(0.0, |sum, cost| sum + cost);
        requests + titles
    }

    /// The typed-in balance less the spend estimated since it was entered.
    #[must_use]
    pub fn manual_balance(&self, manual: &ManualBalance, now: DateTime<Utc>) -> DeepSeekBalance {
        let spent = self.spend(manual.recorded_at, now);
        DeepSeekBalance {
            total: manual.amount - spent,
            topped_up: None,
            granted: None,
            origin: BalanceOrigin::Manual,
            as_of: now,
            spent_since: Some(spent),
        }
    }
}

fn day_start<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .and_then(|naive| tz.from_local_datetime(&naive).earliest())
        .map_or(DateTime::<Utc>::MIN_UTC, |at| at.with_timezone(&Utc))
}

#[derive(Clone, Debug, Serialize)]
pub struct DeepSeekOverview {
    pub today_cost: f64,
    pub today_requests: u64,
    /// Average spend per day over the last [`PACE_DAYS`] days that had any.
    pub daily_average: f64,
    pub days_left: Option<f64>,
    pub balance: Option<DeepSeekBalance>,
    pub low_balance: bool,
    /// Requests whose tokens are counted but which no official price covers.
    pub unpriced_requests: u64,
}

#[must_use]
pub fn deepseek_overview<Tz: TimeZone>(
    costing: &DeepSeekCosting<'_>,
    balance: Option<DeepSeekBalance>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> DeepSeekOverview {
    let today = now.with_timezone(tz).date_naive();
    let today_start = day_start(today, tz);
    let pace_start = day_start(today - Duration::days(PACE_DAYS - 1), tz);
    let mut today_requests = 0;
    let mut unpriced_requests = 0;
    let mut first_day = None;
    for event in &costing.scan.events {
        if event.timestamp > now {
            continue;
        }
        if event.timestamp >= today_start {
            today_requests += 1;
        }
        if event.timestamp >= pace_start {
            first_day = first_day.or(Some(event.timestamp.with_timezone(tz).date_naive()));
            if costing.event_cost(event).is_none() {
                unpriced_requests += 1;
            }
        }
    }
    let today_cost = costing.spend(today_start, now);
    // Divide by the days since the first request in the window, so a user who
    // started yesterday is not paced as if the other five days were free.
    let days = first_day.map_or(1, |first| (today - first).num_days() + 1);
    let daily_average = costing.spend(pace_start, now) / days.clamp(1, PACE_DAYS) as f64;
    let days_left = balance
        .as_ref()
        .filter(|_| daily_average > 0.0)
        .map(|balance| (balance.total / daily_average).max(0.0));
    let low_balance = balance.as_ref().is_some_and(|balance| balance.total <= 0.0)
        || days_left.is_some_and(|days| days <= LOW_BALANCE_DAYS);
    DeepSeekOverview {
        today_cost,
        today_requests,
        daily_average,
        days_left,
        balance,
        low_balance,
        unpriced_requests,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekPeriod {
    Today,
    Week,
    Month,
}

#[derive(Clone, Debug, Serialize)]
pub struct CostRank {
    pub key: String,
    pub name: String,
    /// The project folder for a session row; empty otherwise.
    pub project_name: String,
    pub tokens: u64,
    pub requests: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CostRequest {
    pub at: DateTime<Utc>,
    pub session_id: String,
    pub title: String,
    pub model: String,
    pub tokens: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeepSeekUsage {
    pub period: DeepSeekPeriod,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Requests and title calls together.
    pub cost: f64,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub cache_hit_rate: Option<f64>,
    pub title_cost: f64,
    pub title_requests: u64,
    pub unpriced_requests: u64,
    pub sessions: Vec<CostRank>,
    pub projects: Vec<CostRank>,
    pub models: Vec<CostRank>,
    pub subagents: Vec<CostRank>,
    pub high_requests: Vec<CostRequest>,
    pub overview: DeepSeekOverview,
}

fn cache_hit_rate(tokens: &TokenCounts) -> Option<f64> {
    let denominator =
        tokens.input as u128 + tokens.cache_read as u128 + tokens.cache_write_unspecified as u128;
    (denominator > 0).then(|| tokens.cache_read as f64 / denominator as f64)
}

#[derive(Default)]
struct Group {
    name: String,
    project_name: String,
    tokens: u64,
    requests: u64,
    cost: f64,
}

fn ranked(groups: BTreeMap<String, Group>) -> Vec<CostRank> {
    let mut rows: Vec<CostRank> = groups
        .into_iter()
        .map(|(key, group)| CostRank {
            key,
            name: group.name,
            project_name: group.project_name,
            tokens: group.tokens,
            requests: group.requests,
            cost: group.cost,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.cost
            .total_cmp(&a.cost)
            .then(b.tokens.cmp(&a.tokens))
            .then(a.key.cmp(&b.key))
    });
    rows
}

#[must_use]
pub fn deepseek_usage<Tz: TimeZone>(
    costing: &DeepSeekCosting<'_>,
    period: DeepSeekPeriod,
    balance: Option<DeepSeekBalance>,
    now: DateTime<Utc>,
    tz: &Tz,
    lang: Lang,
) -> DeepSeekUsage {
    let subagent_names = [
        lang.pick("主对话", "Main conversation"),
        lang.pick("子代理", "Subagent"),
    ];
    let today = now.with_timezone(tz).date_naive();
    let first = match period {
        DeepSeekPeriod::Today => today,
        DeepSeekPeriod::Week => today - Duration::days(PACE_DAYS - 1),
        DeepSeekPeriod::Month => today.with_day(1).unwrap_or(today),
    };
    let start = day_start(first, tz);
    let titles = &costing.scan.titles;
    let mut tokens = TokenCounts::default();
    let mut requests = 0;
    let mut unpriced_requests = 0;
    let mut sessions: BTreeMap<String, Group> = BTreeMap::new();
    let mut projects: BTreeMap<String, Group> = BTreeMap::new();
    let mut models: BTreeMap<String, Group> = BTreeMap::new();
    let mut subagents: BTreeMap<String, Group> = BTreeMap::new();
    let mut high_requests = Vec::new();
    for event in &costing.scan.events {
        if event.timestamp < start || event.timestamp > now {
            continue;
        }
        let cost = costing.event_cost(event);
        if cost.is_none() {
            unpriced_requests += 1;
        }
        let cost = cost.unwrap_or(0.0);
        let total = event.tokens.total_without_reasoning();
        tokens.add_saturating(&event.tokens);
        requests += 1;
        let project = event.project.as_deref().unwrap_or_default();
        let title = session_display_name(
            &event.session_id,
            titles.get(&event.session_id).map(String::as_str),
            event.project.as_deref(),
        );
        let add = |group: &mut Group| {
            group.tokens = group.tokens.saturating_add(total);
            group.requests += 1;
            group.cost += cost;
        };
        let session = sessions.entry(event.session_id.clone()).or_default();
        session.name.clone_from(&title);
        session.project_name = directory_name(project);
        add(session);
        let project_group = projects.entry(project.to_owned()).or_default();
        project_group.name = directory_name(project);
        add(project_group);
        let model = models.entry(event.model.clone()).or_default();
        model.name.clone_from(&event.model);
        add(model);
        let kind = if event.is_subagent {
            "subagent"
        } else {
            "main"
        };
        let subagent = subagents.entry(kind.to_owned()).or_default();
        subagent.name = subagent_names[usize::from(event.is_subagent)].to_owned();
        add(subagent);
        high_requests.push(CostRequest {
            at: event.timestamp,
            session_id: event.session_id.clone(),
            title,
            model: event.model.clone(),
            tokens: total,
            cost,
        });
    }
    high_requests.sort_by(|a, b| b.cost.total_cmp(&a.cost).then(b.at.cmp(&a.at)));
    high_requests.truncate(10);
    let mut title_cost = 0.0;
    let mut title_requests = 0;
    for call in &costing.scan.title_calls {
        if call.timestamp >= start && call.timestamp <= now {
            title_requests += 1;
            title_cost += costing.title_cost(call).unwrap_or(0.0);
        }
    }
    let request_cost = sessions.values().fold(0.0, |sum, group| sum + group.cost);
    DeepSeekUsage {
        period,
        start,
        end: now,
        cost: request_cost + title_cost,
        requests,
        cache_hit_rate: cache_hit_rate(&tokens),
        tokens,
        title_cost,
        title_requests,
        unpriced_requests,
        sessions: ranked(sessions),
        projects: ranked(projects),
        models: ranked(models),
        subagents: ranked(subagents),
        high_requests,
        overview: deepseek_overview(costing, balance, now, tz),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekTipKind {
    /// Harness uploads session logs to DeepSeek by default.
    LogUpload,
    /// Harness spends a request on naming each new session.
    TitleRequests,
    /// The first request of a short session pays full price for its prefix.
    ColdStart,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeepSeekTip {
    pub key: String,
    pub kind: DeepSeekTipKind,
    pub sessions: u64,
    pub requests: u64,
    pub tokens: u64,
    pub cost_per_day: f64,
    pub saving_per_day: f64,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeepSeekAdvice {
    pub period_days: u32,
    pub saving_per_day: f64,
    pub sessions: u64,
    pub tips: Vec<DeepSeekTip>,
}

/// A short session has at most this many requests; continuing in an existing
/// session instead would have read most of its first request from the cache.
pub const SHORT_SESSION_REQUESTS: usize = 3;
/// A first request smaller than this is not worth a tip.
pub const COLD_START_MIN_INPUT: u64 = 2_000;

/// Tips for the last `period_days`; dismissed keys are left out.
#[must_use]
pub fn deepseek_advice(
    costing: &DeepSeekCosting<'_>,
    period_days: u32,
    dismissed: &[String],
    now: DateTime<Utc>,
) -> DeepSeekAdvice {
    let since = now - Duration::days(i64::from(period_days));
    let days = f64::from(period_days.max(1));
    let pricing = costing.pricing;
    let mut active = BTreeSet::new();
    let mut per_session: BTreeMap<&str, Vec<&UsageEvent>> = BTreeMap::new();
    for event in &costing.scan.events {
        if event.timestamp >= since && event.timestamp <= now {
            active.insert(event.session_id.as_str());
            per_session
                .entry(event.session_id.as_str())
                .or_default()
                .push(event);
        }
    }
    let mut tips = Vec::new();
    let uploaded = costing
        .scan
        .uploads
        .keys()
        .filter(|session| active.contains(session.as_str()))
        .count() as u64;
    if uploaded > 0 {
        tips.push(DeepSeekTip {
            key: "deepseek:log_upload".into(),
            kind: DeepSeekTipKind::LogUpload,
            sessions: uploaded,
            requests: 0,
            tokens: 0,
            cost_per_day: 0.0,
            saving_per_day: 0.0,
            model: None,
        });
    }
    let calls: Vec<&DeepSeekTitleCall> = costing
        .scan
        .title_calls
        .iter()
        .filter(|call| call.timestamp >= since && call.timestamp <= now)
        .collect();
    if !calls.is_empty() {
        let cost: f64 = calls
            .iter()
            .filter_map(|call| costing.title_cost(call))
            .fold(0.0, |sum, cost| sum + cost);
        let mut models: BTreeMap<&str, u64> = BTreeMap::new();
        for call in &calls {
            *models.entry(call.model.as_str()).or_default() += 1;
        }
        let model = models
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(model, _)| model.to_owned())
            .filter(|model| !model.is_empty());
        let sessions = calls
            .iter()
            .map(|call| call.session_id.as_str())
            .collect::<BTreeSet<_>>()
            .len() as u64;
        tips.push(DeepSeekTip {
            key: "deepseek:title_requests".into(),
            kind: DeepSeekTipKind::TitleRequests,
            sessions,
            requests: calls.len() as u64,
            tokens: calls
                .iter()
                .map(|call| pricing.title_tokens(call.bytes).total_without_reasoning())
                .sum(),
            cost_per_day: cost / days,
            saving_per_day: cost / days,
            model,
        });
    }
    let mut cold_sessions = 0;
    let mut cold_tokens = 0;
    let mut cold_saving = 0.0;
    for events in per_session.values() {
        let Some(first) = events.iter().min_by_key(|event| event.timestamp) else {
            continue;
        };
        // Only sessions that started in the period: an older one's first request is not here.
        let started_here = costing
            .by_session
            .get(first.session_id.as_str())
            .is_some_and(|all| all.iter().all(|event| event.timestamp >= first.timestamp));
        if !started_here
            || events.len() > SHORT_SESSION_REQUESTS
            || first.tokens.cache_read > 0
            || first.tokens.input < COLD_START_MIN_INPUT
        {
            continue;
        }
        cold_sessions += 1;
        cold_tokens += first.tokens.input;
        let cached = TokenCounts {
            cache_read: first.tokens.input,
            ..first.tokens
        };
        let uncached = costing.event_cost(first);
        let at_cache_price = pricing.cost_cny(
            &first.model,
            &TokenCounts { input: 0, ..cached },
            first.timestamp,
        );
        if let (Some(full), Some(cheap)) = (uncached, at_cache_price) {
            cold_saving += (full - cheap).max(0.0);
        }
    }
    if cold_sessions > 0 {
        tips.push(DeepSeekTip {
            key: "deepseek:cold_start".into(),
            kind: DeepSeekTipKind::ColdStart,
            sessions: cold_sessions,
            requests: cold_sessions,
            tokens: cold_tokens,
            cost_per_day: 0.0,
            saving_per_day: cold_saving / days,
            model: None,
        });
    }
    tips.retain(|tip| !dismissed.contains(&tip.key));
    DeepSeekAdvice {
        period_days,
        saving_per_day: tips.iter().fold(0.0, |sum, tip| sum + tip.saving_per_day),
        sessions: active.len() as u64,
        tips,
    }
}
