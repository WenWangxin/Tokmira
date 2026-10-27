//! Queries Claude's official quota after new local Claude usage, whether or not the window
//! is open, so the window and the menu bar catch up within the minimum interval. A burst
//! of requests gets one query right away and one more when the interval ends, so the last
//! request of the burst is not left waiting for the next periodic poll. Same rules as every
//! automatic query: only with the switch on, never a keychain prompt, rate-limit backoff.

use std::sync::mpsc::{self, Sender, TryRecvError};

use chrono::{DateTime, Duration, Utc};
use tauri::{AppHandle, Manager};
use token_audit_core::settings;

use crate::{AppState, RefreshClock};

/// Minimum time between two Claude queries; the same limit `claude_skip_reason` applies.
const INTERVAL_SECONDS: i64 = 60;

/// How long to wait before the next automatic query may run, or `None` while automatic
/// queries wait for the user (a denied keychain read).
fn follow_delay(clock: &RefreshClock, now: DateTime<Utc>) -> Option<Duration> {
    if clock.claude_awaits_user {
        return None;
    }
    let ready = [
        clock.claude_retry_at,
        clock
            .last_claude
            .map(|at| at + Duration::seconds(INTERVAL_SECONDS)),
    ]
    .into_iter()
    .flatten()
    .fold(now, DateTime::max);
    Some(ready - now)
}

/// Starts the follow-up thread; send `()` whenever the Claude logs changed.
pub fn spawn(app: AppHandle) -> Sender<()> {
    let (sender, receiver) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        while receiver.recv().is_ok() {
            loop {
                let state = app.state::<AppState>();
                let enabled = settings::read_settings(&state.data_dir)
                    .is_ok_and(|current| current.claude_official_query);
                let delay = state
                    .refresh
                    .lock()
                    .ok()
                    .and_then(|clock| follow_delay(&clock, Utc::now()));
                let (true, Some(delay)) = (enabled, delay) else {
                    break;
                };
                std::thread::sleep(delay.to_std().unwrap_or_default());
                // This query covers every change so far.
                while receiver.try_recv().is_ok() {}
                let _ = crate::refresh_claude_sync(&state);
                // Usage logged while the query ran gets one more query after the interval.
                match receiver.try_recv() {
                    Ok(()) => continue,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
        }
    });
    sender
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_follow_up_waits_out_the_interval_and_any_backoff() {
        let now = Utc::now();
        let mut clock = RefreshClock::default();
        assert_eq!(follow_delay(&clock, now), Some(Duration::zero()));

        clock.last_claude = Some(now - Duration::seconds(20));
        assert_eq!(follow_delay(&clock, now), Some(Duration::seconds(40)));

        clock.claude_retry_at = Some(now + Duration::minutes(5));
        assert_eq!(follow_delay(&clock, now), Some(Duration::minutes(5)));

        // A denied keychain read stops automatic queries until the user acts.
        clock.claude_awaits_user = true;
        assert_eq!(follow_delay(&clock, now), None);
    }
}
