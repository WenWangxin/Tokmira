use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::model::Tool;
use crate::timeline::{window_id, QuotaPoint, Window};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Forecast {
    WillHit { at: DateTime<Utc> },
    Ample,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    FiveHour,
    Weekly,
}
fn selected(point: &QuotaPoint, kind: WindowKind) -> Option<&Window> {
    match kind {
        WindowKind::FiveHour => point.five_hour.as_ref(),
        WindowKind::Weekly => point.weekly.as_ref(),
    }
}

pub fn forecast(
    points: &[QuotaPoint],
    tool: Tool,
    kind: WindowKind,
    now: DateTime<Utc>,
) -> Forecast {
    let mut series: Vec<_> = points
        .iter()
        .filter(|p| p.tool == tool && p.at <= now)
        .filter_map(|p| Some((p.at, selected(p, kind)?)))
        .collect();
    series.sort_by_key(|(at, _)| *at);
    let Some((_, latest)) = series.last().copied() else {
        return Forecast::Unknown;
    };
    let Some(reset) = latest.resets_at else {
        return Forecast::Unknown;
    };
    if reset <= now {
        return Forecast::Unknown;
    }
    let id = window_id(latest);
    series.retain(|(_, w)| window_id(w) == id);
    let Some(first_reset) = series.iter().filter_map(|(_, w)| w.resets_at).min() else {
        return Forecast::Unknown;
    };
    let minutes = latest.window_minutes.unwrap_or(match kind {
        WindowKind::FiveHour => 300,
        WindowKind::Weekly => 10080,
    });
    let Some(start) = first_reset.checked_sub_signed(Duration::minutes(minutes as i64)) else {
        return Forecast::Unknown;
    };
    let from = (now - Duration::minutes(30)).max(start);
    if from >= now {
        return Forecast::Unknown;
    }
    let from_percent = series
        .iter()
        .rev()
        .find(|(at, _)| *at <= from && *at >= start)
        .map_or(0.0, |(_, window)| window.used_percent);
    let minutes = (now - from).num_milliseconds() as f64 / 60_000.0;
    let rate = (latest.used_percent - from_percent) / minutes;
    if rate <= 0.0 {
        return Forecast::Ample;
    }
    let remaining = (100.0 - latest.used_percent).max(0.0) / rate;
    let Some(hit) = now.checked_add_signed(Duration::milliseconds((remaining * 60_000.0) as i64))
    else {
        return Forecast::Unknown;
    };
    if hit < reset {
        Forecast::WillHit { at: hit }
    } else {
        Forecast::Ample
    }
}
