//! Scheduling for the in-app update check. The check itself runs in the desktop app;
//! these rules decide when it may go online and when a found version is worth a notice.

use chrono::{DateTime, Duration, Utc};

/// Automatic checks go online at most once per this many hours.
pub const AUTO_CHECK_INTERVAL_HOURS: i64 = 24;

/// Whether the background check may run now. Off unless the user enabled it; a clock
/// that went backwards counts as not due, so a wrong date cannot cause repeated checks.
pub fn auto_check_due(
    enabled: bool,
    last_checked: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    enabled
        && last_checked
            .is_none_or(|at| at <= now && now - at >= Duration::hours(AUTO_CHECK_INTERVAL_HOURS))
}

/// Each new version is announced once, however many checks find it.
pub fn should_notify(version: &str, notified: Option<&str>) -> bool {
    notified != Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn automatic_checks_wait_for_the_switch_and_a_full_day() {
        let now = at("2026-10-27T12:00:00Z");
        assert!(!auto_check_due(false, None, now));
        assert!(auto_check_due(true, None, now));
        assert!(!auto_check_due(true, Some(at("2026-10-27T00:00:01Z")), now));
        assert!(auto_check_due(true, Some(at("2026-10-26T12:00:00Z")), now));
        // A last check "in the future" means the clock moved back; wait instead of looping.
        assert!(!auto_check_due(true, Some(at("2026-10-28T00:00:00Z")), now));
    }

    #[test]
    fn a_version_is_announced_once() {
        assert!(should_notify("0.2.0", None));
        assert!(!should_notify("0.2.0", Some("0.2.0")));
        assert!(should_notify("0.2.1", Some("0.2.0")));
    }
}
