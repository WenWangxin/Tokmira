use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;

use crate::codex::RateLimitSnapshot;
use crate::model::Tool;
use crate::quota_store::{ClaudeSnapshot, CodexSnapshot, StoredWindow};
use crate::recorder::StatuslineRecord;
use crate::settings::Calibration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Origin {
    Official,
    Statusline,
    Log,
    Manual,
    Estimated,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Window {
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QuotaPoint {
    pub tool: Tool,
    pub at: DateTime<Utc>,
    pub origin: Origin,
    pub five_hour: Option<Window>,
    pub weekly: Option<Window>,
}

pub fn window_id(window: &Window) -> Option<DateTime<Utc>> {
    let resets = window.resets_at?;
    Utc.timestamp_opt(resets.timestamp().div_euclid(60) * 60, 0)
        .single()
}
fn stored(window: &StoredWindow) -> Window {
    Window {
        used_percent: window.used_percent,
        resets_at: window.resets_at,
        window_minutes: window.window_minutes,
    }
}
fn same_window(a: &Option<Window>, b: &Option<Window>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.used_percent == b.used_percent && window_id(a) == window_id(b),
        (None, None) => true,
        _ => false,
    }
}

/// Points are sorted stably by tool and timestamp. An unchanged sample in the same
/// reset window adds no information; a new reset window always starts a new series.
pub fn build_timeline(
    claude: &[ClaudeSnapshot],
    codex: &[CodexSnapshot],
    statusline: &[StatuslineRecord],
    log: &[RateLimitSnapshot],
    calibration: Option<&Calibration>,
) -> Vec<QuotaPoint> {
    let mut points = Vec::new();
    for record in claude {
        points.push(QuotaPoint {
            tool: Tool::Claude,
            at: record.at,
            origin: Origin::Official,
            five_hour: record.five_hour.as_ref().map(stored),
            weekly: record.seven_day.as_ref().map(stored),
        });
    }
    for record in codex {
        if !matches!(record.limit_id.as_deref(), None | Some("codex")) {
            continue;
        }
        points.push(QuotaPoint {
            tool: Tool::Codex,
            at: record.at,
            origin: Origin::Official,
            five_hour: record.primary.as_ref().map(stored),
            weekly: record.secondary.as_ref().map(stored),
        });
    }
    let status_window = |w: &crate::recorder::QuotaWindow, minutes| {
        w.used_percentage.map(|used_percent| Window {
            used_percent,
            resets_at: w.resets_at.and_then(DateTime::<Utc>::from_timestamp_secs),
            window_minutes: Some(minutes),
        })
    };
    for record in statusline {
        points.push(QuotaPoint {
            tool: Tool::Claude,
            at: record.recorded_at,
            origin: Origin::Statusline,
            five_hour: record
                .five_hour
                .as_ref()
                .and_then(|w| status_window(w, 300)),
            weekly: record
                .seven_day
                .as_ref()
                .and_then(|w| status_window(w, 10080)),
        });
    }
    let rate_window = |w: &crate::codex::RateLimitWindow| Window {
        used_percent: w.used_percent,
        resets_at: w.resets_at,
        window_minutes: w.window_minutes,
    };
    for record in log {
        if !matches!(record.limit_id.as_deref(), None | Some("codex")) {
            continue;
        }
        points.push(QuotaPoint {
            tool: Tool::Codex,
            at: record.timestamp,
            origin: Origin::Log,
            five_hour: record.primary.as_ref().map(rate_window),
            weekly: record.secondary.as_ref().map(rate_window),
        });
    }
    if let Some(record) = calibration {
        points.push(QuotaPoint {
            tool: record.tool,
            at: record.recorded_at,
            origin: Origin::Manual,
            five_hour: Some(Window {
                used_percent: record.five_hour_percent,
                resets_at: None,
                window_minutes: Some(300),
            }),
            weekly: None,
        });
    }
    points.sort_by_key(|point| (point.tool, point.at));
    points.dedup_by(|a, b| {
        a.tool == b.tool
            && same_window(&a.five_hour, &b.five_hour)
            && same_window(&a.weekly, &b.weekly)
    });
    points
}
