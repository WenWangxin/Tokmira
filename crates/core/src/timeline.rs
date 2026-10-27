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

/// Tolerance when telling a not-yet-started window by its sliding reset time.
const IDLE_SLACK_SECONDS: i64 = 120;

/// A window nobody has used yet reports its reset as "sample time + window length",
/// which slides forward with every sample and names no real window. That reset is
/// dropped; once usage starts the reset time stops moving and is kept as is.
fn forget_sliding_reset(window: &mut Window, at: DateTime<Utc>) {
    let (Some(reset), Some(minutes)) = (window.resets_at, window.window_minutes) else {
        return;
    };
    if window.used_percent > 0.0 {
        return;
    }
    let length = i64::try_from(minutes)
        .unwrap_or(i64::MAX)
        .saturating_mul(60);
    if ((reset - at).num_seconds() - length).abs() <= IDLE_SLACK_SECONDS {
        window.resets_at = None;
    }
}

/// Reset times of one window differ by a fraction of a second between samples and
/// sources (Claude reports `13:49:59.5` and `13:50:00.1` for the same window), which
/// can straddle a minute and split the window in two. Within one tool and window
/// kind, a reset close to the previous one is replaced by it. Expects points sorted
/// by tool and time.
fn unify_jittered_resets(points: &mut [QuotaPoint]) {
    let mut tool = None;
    let mut last: [Option<DateTime<Utc>>; 2] = [None, None];
    for point in points {
        if tool != Some(point.tool) {
            tool = Some(point.tool);
            last = [None, None];
        }
        for (slot, window) in [&mut point.five_hour, &mut point.weekly]
            .into_iter()
            .enumerate()
        {
            let Some(reset) = window.as_mut().and_then(|w| w.resets_at.as_mut()) else {
                continue;
            };
            match last[slot] {
                Some(previous) if (*reset - previous).num_seconds().abs() <= IDLE_SLACK_SECONDS => {
                    *reset = previous;
                }
                _ => last[slot] = Some(*reset),
            }
        }
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
    for point in &mut points {
        let at = point.at;
        for window in [&mut point.five_hour, &mut point.weekly]
            .into_iter()
            .flatten()
        {
            forget_sliding_reset(window, at);
        }
    }
    points.sort_by_key(|point| (point.tool, point.at));
    unify_jittered_resets(&mut points);
    points.dedup_by(|a, b| {
        a.tool == b.tool
            && same_window(&a.five_hour, &b.five_hour)
            && same_window(&a.weekly, &b.weekly)
    });
    points
}
