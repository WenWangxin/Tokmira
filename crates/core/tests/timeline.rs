use chrono::{DateTime, Duration, Utc};
use token_audit_core::quota_store::{CodexSnapshot, StoredWindow};
use token_audit_core::timeline::build_timeline;

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("time")
        .with_timezone(&Utc)
}

fn snapshot(time: DateTime<Utc>, used: f64, reset: DateTime<Utc>) -> CodexSnapshot {
    CodexSnapshot {
        at: time,
        limit_id: Some("codex".to_owned()),
        plan_type: Some("plus".to_owned()),
        primary: Some(StoredWindow {
            used_percent: used,
            resets_at: Some(reset),
            window_minutes: Some(300),
        }),
        secondary: Some(StoredWindow {
            used_percent: 16.0,
            resets_at: Some(at("2026-11-01T09:00:14Z")),
            window_minutes: Some(10080),
        }),
        reset_credits_available: None,
    }
}

#[test]
fn an_unused_window_does_not_become_a_new_window_every_poll() {
    // Polled every minute while nobody uses Codex: the reported reset slides along.
    let start = at("2026-10-25T14:06:03Z");
    let mut snapshots: Vec<_> = (0..8)
        .map(|minute| {
            let time = start + Duration::seconds(minute * 75);
            snapshot(
                time,
                0.0,
                time + Duration::minutes(300) - Duration::seconds(1),
            )
        })
        .collect();
    // Then a request starts the window; its reset stops moving.
    let first_use = start + Duration::minutes(20);
    let real_reset = first_use + Duration::minutes(300);
    snapshots.push(snapshot(first_use, 3.0, real_reset));
    snapshots.push(snapshot(first_use + Duration::minutes(5), 7.0, real_reset));

    let points = build_timeline(&[], &snapshots, &[], &[], None);
    let resets: Vec<_> = points
        .iter()
        .map(|point| point.five_hour.as_ref().and_then(|w| w.resets_at))
        .collect();
    // The idle samples collapse into one point with no reset; the used window keeps its own.
    assert_eq!(resets, vec![None, Some(real_reset), Some(real_reset)]);
    // The weekly window, which is in use, is untouched.
    assert!(points
        .iter()
        .all(|point| point.weekly.as_ref().and_then(|w| w.resets_at).is_some()));
}

#[test]
fn a_started_window_at_zero_percent_keeps_its_reset() {
    // Used a little but still rounding to 0%: the reset no longer tracks the sample time.
    let started = at("2026-10-25T10:00:00Z");
    let reset = started + Duration::minutes(300);
    let later = started + Duration::minutes(40);
    let points = build_timeline(&[], &[snapshot(later, 0.0, reset)], &[], &[], None);
    assert_eq!(
        points[0].five_hour.as_ref().and_then(|w| w.resets_at),
        Some(reset)
    );
}

#[test]
fn reset_jitter_across_a_minute_keeps_one_window() {
    use token_audit_core::quota_store::ClaudeSnapshot;
    let claude = |time: &str, used: f64, reset: &str| -> ClaudeSnapshot {
        serde_json::from_value(serde_json::json!({
            "at": time,
            "subscription_type": null,
            "five_hour": { "used_percent": used, "resets_at": reset, "window_minutes": 300 },
            "seven_day": { "used_percent": 50.0, "resets_at": "2026-10-28T18:00:00.188460Z", "window_minutes": 10080 }
        }))
        .expect("snapshot")
    };
    let snapshots = [
        claude("2026-10-25T11:03:22Z", 41.0, "2026-10-25T13:50:00.403718Z"),
        claude("2026-10-25T12:32:27Z", 60.0, "2026-10-25T13:49:59.535788Z"),
        claude("2026-10-25T13:10:41Z", 68.0, "2026-10-25T13:50:00.062937Z"),
        // The next window is a different reset entirely and stays separate.
        claude("2026-10-25T14:00:00Z", 2.0, "2026-10-25T18:55:00Z"),
    ];
    let points = build_timeline(&snapshots, &[], &[], &[], None);
    let resets: Vec<_> = points
        .iter()
        .filter_map(|point| point.five_hour.as_ref().and_then(|w| w.resets_at))
        .collect();
    let first = at("2026-10-25T13:50:00.403718Z");
    assert_eq!(
        resets,
        vec![first, first, first, at("2026-10-25T18:55:00Z")]
    );
    let ids: std::collections::BTreeSet<_> = points
        .iter()
        .filter_map(|point| {
            point
                .five_hour
                .as_ref()
                .and_then(token_audit_core::timeline::window_id)
        })
        .collect();
    assert_eq!(ids.len(), 2);
}
