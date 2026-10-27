//! Fills the gap between two quota readings: requests logged after the latest reading are
//! added at a percent-per-dollar rate, and the next reading replaces the estimate. The rate
//! comes from the current window (its reading over the local spend since the window began)
//! when that window has seen enough spend, otherwise from the last two weeks' attribution.
//! Backtested on real Codex readings 2–4 minutes apart, the window rate missed by about
//! 0.7 points on the 5-hour window, the two-week rate alone by about 1.2 (both high).
//! Only usage fields and timestamps are used.

use chrono::{DateTime, Duration, Utc};

use crate::attribution::{EventAttribution, WindowKind};
use crate::model::{Tool, UsageEvent};
use crate::pricing::Pricing;

/// How far back requests are used to learn the rate.
pub const LEARN_DAYS: i64 = 14;
/// Below this much attributed spend the rate is too noisy to show.
pub const MIN_LEARN_COST_USD: f64 = 1.0;
pub const MIN_LEARN_REQUESTS: usize = 5;

fn cost(pricing: &Pricing, event: &UsageEvent) -> f64 {
    pricing
        .cost_usd(&event.model, &event.tokens)
        .unwrap_or(0.0)
        .max(0.0)
}

/// Percent per USD within the current window: the latest reading over the local spend
/// since the window began. `None` until the window has seen `MIN_LEARN_COST_USD`.
pub fn window_rate(
    events: &[UsageEvent],
    pricing: &Pricing,
    tool: Tool,
    used_percent: f64,
    window_start: DateTime<Utc>,
    reading_at: DateTime<Utc>,
) -> Option<f64> {
    let spent = cost_since(events, pricing, tool, window_start, reading_at);
    (spent >= MIN_LEARN_COST_USD && used_percent > 0.0).then(|| used_percent / spent)
}

/// Percent of the window per USD of API-equivalent cost, from requests up to `until`
/// that received a share of an observed increase. `None` without enough history.
pub fn learn_rate(
    events: &[EventAttribution],
    pricing: &Pricing,
    tool: Tool,
    kind: WindowKind,
    until: DateTime<Utc>,
) -> Option<f64> {
    let since = until - Duration::days(LEARN_DAYS);
    let (mut percent, mut spent, mut requests) = (0.0, 0.0, 0_usize);
    for item in events.iter().filter(|item| {
        item.event.tool == tool && item.event.timestamp > since && item.event.timestamp <= until
    }) {
        let delta = match kind {
            WindowKind::FiveHour => item.five_hour_delta,
            WindowKind::Weekly => item.weekly_delta,
        };
        let price = cost(pricing, &item.event);
        if delta <= 0.0 || price <= 0.0 {
            continue;
        }
        percent += delta;
        spent += price;
        requests += 1;
    }
    (requests >= MIN_LEARN_REQUESTS && spent >= MIN_LEARN_COST_USD).then(|| percent / spent)
}

/// API-equivalent cost of the tool's requests logged after `after`, up to `now`.
pub fn cost_since(
    events: &[UsageEvent],
    pricing: &Pricing,
    tool: Tool,
    after: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f64 {
    events
        .iter()
        .filter(|event| event.tool == tool && event.timestamp > after && event.timestamp <= now)
        .map(|event| cost(pricing, event))
        .sum()
}
