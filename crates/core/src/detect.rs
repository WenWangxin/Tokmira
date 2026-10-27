//! Which local tools have usage logs, for the first-run guide.
//!
//! Counts every historical request. Plan names come from the newest saved
//! Claude quota snapshot and the newest Codex log snapshot, not from message text.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::codex::RateLimitSnapshot;
use crate::model::{Source, UsageEvent};
use crate::quota_store::{normalize_claude_plan, ClaudeSnapshot};

/// One of the four log sources the guide can show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DetectedTool {
    pub source: Source,
    /// Distinct session ids across the whole history.
    pub sessions: usize,
    /// Usage events, including subagent requests.
    pub requests: usize,
    pub last_used_at: Option<DateTime<Utc>>,
    /// Claude: recognized Max tiers are normalized from the newest saved snapshot.
    /// Codex: `plan_type` on the newest log snapshot. Empty values are omitted.
    pub plan: Option<String>,
}

const SOURCES: [Source; 4] = [
    Source::ClaudeCli,
    Source::ClaudeDesktop,
    Source::CodexCli,
    Source::CodexDesktop,
];

#[derive(Default)]
struct SourceCounts {
    sessions: BTreeSet<String>,
    requests: usize,
    last_used_at: Option<DateTime<Utc>>,
}

/// Summarise the four sources. Unknown events are ignored. The result is always four rows,
/// in the order Claude command line, Claude desktop, Codex command line, Codex desktop.
#[must_use]
pub fn detect_tools(
    events: &[UsageEvent],
    codex_log: &[RateLimitSnapshot],
    claude_snapshots: &[ClaudeSnapshot],
) -> Vec<DetectedTool> {
    let mut counts = [
        SourceCounts::default(),
        SourceCounts::default(),
        SourceCounts::default(),
        SourceCounts::default(),
    ];
    for event in events {
        let Some(index) = source_index(event.source) else {
            continue;
        };
        let bucket = &mut counts[index];
        bucket.sessions.insert(event.session_id.clone());
        bucket.requests += 1;
        if bucket.last_used_at.is_none_or(|at| event.timestamp > at) {
            bucket.last_used_at = Some(event.timestamp);
        }
    }
    let claude_plan = claude_snapshots
        .iter()
        .max_by_key(|snapshot| snapshot.at)
        .and_then(|snapshot| {
            normalize_claude_plan(
                snapshot.subscription_type.as_deref(),
                snapshot.rate_limit_tier.as_deref(),
            )
        });
    let codex_plan = meaningful_plan(
        codex_log
            .iter()
            .max_by_key(|snapshot| snapshot.timestamp)
            .and_then(|snapshot| snapshot.plan_type.as_deref()),
    );
    SOURCES
        .into_iter()
        .zip(counts)
        .map(|(source, bucket)| DetectedTool {
            plan: if source_index(source).is_some_and(|index| index < 2) {
                claude_plan.clone()
            } else {
                codex_plan.clone()
            },
            source,
            sessions: bucket.sessions.len(),
            requests: bucket.requests,
            last_used_at: bucket.last_used_at,
        })
        .collect()
}

fn source_index(source: Source) -> Option<usize> {
    SOURCES.iter().position(|item| *item == source)
}

fn meaningful_plan(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{detect_tools, DetectedTool};
    use crate::codex::RateLimitSnapshot;
    use crate::model::{Source, TokenCounts, Tool, UsageEvent};
    use crate::quota_store::ClaudeSnapshot;

    fn at(timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(timestamp)
            .expect("rfc3339")
            .with_timezone(&Utc)
    }

    fn event(source: Source, session: &str, timestamp: &str, subagent: bool) -> UsageEvent {
        UsageEvent {
            tool: match source {
                Source::CodexCli | Source::CodexDesktop => Tool::Codex,
                _ => Tool::Claude,
            },
            source,
            session_id: session.to_owned(),
            request_id: None,
            timestamp: at(timestamp),
            model: "model".to_owned(),
            project: None,
            is_subagent: subagent,
            fast: false,
            effort: None,
            web_search_requests: 0,
            tokens: TokenCounts::default(),
        }
    }

    fn claude_snapshot(timestamp: &str, plan: Option<&str>) -> ClaudeSnapshot {
        ClaudeSnapshot {
            at: at(timestamp),
            subscription_type: plan.map(str::to_owned),
            rate_limit_tier: None,
            five_hour: None,
            seven_day: None,
            weekly_breakdown: None,
        }
    }

    fn codex_snapshot(timestamp: &str, plan: Option<&str>) -> RateLimitSnapshot {
        RateLimitSnapshot {
            timestamp: at(timestamp),
            session_id: "session".to_owned(),
            source: Source::CodexCli,
            limit_id: None,
            plan_type: plan.map(str::to_owned),
            primary: None,
            secondary: None,
        }
    }

    fn row(tools: &[DetectedTool], source: Source) -> &DetectedTool {
        tools
            .iter()
            .find(|tool| tool.source == source)
            .expect("source row")
    }

    #[test]
    fn counts_sessions_requests_and_latest_use_per_source() {
        let events = vec![
            event(Source::ClaudeCli, "a", "2026-10-24T10:00:00Z", false),
            event(Source::ClaudeCli, "a", "2026-10-24T08:00:00Z", false),
            event(Source::ClaudeCli, "b", "2026-10-24T09:00:00Z", true),
            event(Source::ClaudeDesktop, "desk", "2026-10-23T01:00:00Z", false),
            event(Source::CodexCli, "c", "2026-10-24T11:00:00Z", false),
            event(Source::CodexDesktop, "d", "2026-10-22T01:00:00Z", false),
            event(Source::CodexDesktop, "d", "2026-10-22T02:00:00Z", false),
            event(Source::Unknown, "ignored", "2026-10-25T00:00:00Z", false),
        ];
        let tools = detect_tools(
            &events,
            &[codex_snapshot("2026-10-01T00:00:00Z", Some("plus"))],
            &[claude_snapshot("2026-10-01T00:00:00Z", Some("pro"))],
        );
        assert_eq!(
            tools.iter().map(|tool| tool.source).collect::<Vec<_>>(),
            vec![
                Source::ClaudeCli,
                Source::ClaudeDesktop,
                Source::CodexCli,
                Source::CodexDesktop,
            ]
        );
        let cli = row(&tools, Source::ClaudeCli);
        assert_eq!(cli.sessions, 2);
        assert_eq!(cli.requests, 3);
        assert_eq!(cli.last_used_at, Some(at("2026-10-24T10:00:00Z")));
        assert_eq!(cli.plan.as_deref(), Some("pro"));
        assert_eq!(
            row(&tools, Source::ClaudeDesktop).plan.as_deref(),
            Some("pro")
        );
        assert_eq!(row(&tools, Source::ClaudeDesktop).sessions, 1);
        assert_eq!(row(&tools, Source::CodexCli).requests, 1);
        assert_eq!(row(&tools, Source::CodexDesktop).sessions, 1);
        assert_eq!(row(&tools, Source::CodexDesktop).requests, 2);
        assert_eq!(row(&tools, Source::CodexCli).plan.as_deref(), Some("plus"));
    }

    #[test]
    fn newest_snapshot_wins_and_blank_plan_is_absent() {
        let tools = detect_tools(
            &[],
            &[
                codex_snapshot("2026-10-02T00:00:00Z", Some("plus")),
                codex_snapshot("2026-10-03T00:00:00Z", Some("  pro  ")),
                codex_snapshot("2026-10-04T00:00:00Z", Some("   ")),
            ],
            &[
                claude_snapshot("2026-10-02T00:00:00Z", Some("pro")),
                claude_snapshot("2026-10-05T00:00:00Z", None),
            ],
        );
        assert!(tools
            .iter()
            .all(|tool| tool.sessions == 0 && tool.requests == 0));
        assert!(tools.iter().all(|tool| tool.last_used_at.is_none()));
        assert!(row(&tools, Source::ClaudeCli).plan.is_none());
        assert!(row(&tools, Source::CodexDesktop).plan.is_none());
    }

    #[test]
    fn equal_timestamps_keep_the_later_snapshot() {
        let tools = detect_tools(
            &[],
            &[
                codex_snapshot("2026-10-04T00:00:00Z", Some("plus")),
                codex_snapshot("2026-10-04T00:00:00Z", Some("pro")),
            ],
            &[
                claude_snapshot("2026-10-04T00:00:00Z", Some("max")),
                claude_snapshot("2026-10-04T00:00:00Z", Some("  pro  ")),
            ],
        );
        assert_eq!(
            row(&tools, Source::ClaudeDesktop).plan.as_deref(),
            Some("pro")
        );
        assert_eq!(row(&tools, Source::CodexCli).plan.as_deref(), Some("pro"));
    }
}
