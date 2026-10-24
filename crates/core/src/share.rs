use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::dashboard::{build_overview, OverviewInput};
use crate::model::{TokenCounts, Tool};
use crate::usage::directory_name;

/// One tool's API-equivalent cost for the local calendar month.
///
/// `plan` is the raw snapshot label. `subscription_usd` is set only when that
/// label maps to a known monthly price.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShareToolCost {
    pub tool: Tool,
    pub plan: Option<String>,
    pub subscription_usd: Option<f64>,
    pub estimated_cost_usd: f64,
}

/// Highest-cost project this month. `name` is the directory name only.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShareProject {
    pub name: String,
    pub estimated_cost_usd: f64,
}

/// Month-to-date figures for the share card. Costs and the value multiple use
/// the same rules as the dashboard overview.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShareSummary {
    pub year: i32,
    pub month: u32,
    pub month_estimated_cost_usd: f64,
    /// Tools with non-zero token usage this month, Claude then Codex.
    pub tools: Vec<ShareToolCost>,
    /// Tools with a known monthly price, including those with no usage this month.
    pub recognized_subscriptions: usize,
    pub subscription_total_usd: Option<f64>,
    /// Cost of recognized tools only, divided by their combined monthly price.
    pub subscription_value_multiple: Option<f64>,
    /// Tools that have usage this month but no known monthly price.
    pub unrecognized_tools: Vec<Tool>,
    pub cache_hit_rate: Option<f64>,
    pub requests: u64,
    pub top_project: Option<ShareProject>,
}

/// Current local calendar month: total API-equivalent cost, per-tool cost and
/// recognized plan, subscription total, value multiple, cache hit rate, request
/// count, and the highest-cost project's directory name.
pub fn share_summary<Tz: TimeZone>(
    input: &OverviewInput<'_>,
    now: DateTime<Utc>,
    tz: Tz,
) -> ShareSummary {
    let today = now.with_timezone(&tz).date_naive();
    let mut month_tokens = TokenCounts::default();
    let mut requests = 0_u64;
    let mut projects: BTreeMap<String, f64> = BTreeMap::new();
    for event in input.events {
        if !in_month(event.timestamp, today, &tz) {
            continue;
        }
        requests = requests.saturating_add(1);
        month_tokens.add_saturating(&event.tokens);
        let Some(project) = event
            .project
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        else {
            continue;
        };
        let name = directory_name(project).trim().to_owned();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            continue;
        }
        let cost = input
            .pricing
            .cost_usd(&event.model, &event.tokens)
            .unwrap_or(0.0);
        *projects.entry(project.to_owned()).or_default() += cost;
    }
    let top_project = projects
        .into_iter()
        .max_by(|(path_a, cost_a), (path_b, cost_b)| {
            cost_a.total_cmp(cost_b).then_with(|| {
                directory_name(path_b)
                    .cmp(&directory_name(path_a))
                    .then_with(|| path_b.cmp(path_a))
            })
        })
        .map(|(path, estimated_cost_usd)| ShareProject {
            name: directory_name(&path).trim().to_owned(),
            estimated_cost_usd,
        });
    let overview = build_overview(input, now, tz);
    let tools: Vec<ShareToolCost> = overview
        .tools
        .iter()
        .filter(|tool| overview.month_cost_by_tool.contains_key(&tool.tool))
        .map(|tool| ShareToolCost {
            tool: tool.tool,
            plan: tool.plan.clone(),
            subscription_usd: tool.subscription_usd,
            estimated_cost_usd: overview
                .month_cost_by_tool
                .get(&tool.tool)
                .copied()
                .unwrap_or(0.0),
        })
        .collect();
    let unrecognized_tools = tools
        .iter()
        .filter(|tool| tool.subscription_usd.is_none())
        .map(|tool| tool.tool)
        .collect();
    let recognized_subscriptions = overview
        .tools
        .iter()
        .filter(|tool| tool.subscription_usd.is_some())
        .count();
    ShareSummary {
        year: today.year(),
        month: today.month(),
        month_estimated_cost_usd: overview.month_estimated_cost_usd,
        tools,
        recognized_subscriptions,
        subscription_total_usd: overview.subscription_total_usd,
        subscription_value_multiple: overview.subscription_value_multiple,
        unrecognized_tools,
        cache_hit_rate: cache_hit_rate(&month_tokens),
        requests,
        top_project,
    }
}

fn in_month<Tz: TimeZone>(timestamp: DateTime<Utc>, today: NaiveDate, tz: &Tz) -> bool {
    let date = timestamp.with_timezone(tz).date_naive();
    date.year() == today.year() && date.month() == today.month()
}

fn cache_hit_rate(tokens: &TokenCounts) -> Option<f64> {
    let denominator = tokens.input as u128
        + tokens.cache_read as u128
        + tokens.cache_write_5m as u128
        + tokens.cache_write_1h as u128
        + tokens.cache_write_unspecified as u128;
    (denominator > 0).then(|| tokens.cache_read as f64 / denominator as f64)
}

/// Writes `bytes` into `directory` under `file_name`.
///
/// Only a single `.png` or `.md` file name is accepted. When that name already
/// exists, `-2`, `-3`, … is appended before the extension.
pub fn save_export_file(
    directory: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<PathBuf, &'static str> {
    let (stem, ext) = export_name_parts(file_name)?;
    fs::create_dir_all(directory).map_err(|_| "保存文件失败")?;
    for index in 1..=10_000 {
        let name = if index == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{index}.{ext}")
        };
        if name.contains('/') || name.contains('\\') {
            return Err("文件名无效");
        }
        let path = directory.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes).map_err(|_| "保存文件失败")?;
                return Ok(path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("保存文件失败"),
        }
    }
    Err("保存文件失败")
}

fn export_name_parts(file_name: &str) -> Result<(String, String), &'static str> {
    if file_name.is_empty()
        || file_name
            .chars()
            .any(|ch| ch.is_control() || ch == '/' || ch == '\\' || ch == ':')
    {
        return Err("文件名无效");
    }
    let path = Path::new(file_name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) if name.to_str() == Some(file_name) => {}
        _ => return Err("文件名无效"),
    }
    let ext = match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext @ ("png" | "md")) => ext.to_owned(),
        Some(_) => return Err("只能导出 PNG 或 Markdown 文件"),
        None => return Err("文件名无效"),
    };
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty() && *stem != "." && *stem != "..")
        .ok_or("文件名无效")?;
    Ok((stem.to_owned(), ext))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{FixedOffset, TimeZone, Utc};
    use serde_json::json;

    use super::{save_export_file, share_summary};
    use crate::attribution::attribute;
    use crate::codex::RateLimitSnapshot;
    use crate::dashboard::OverviewInput;
    use crate::model::{Source, TokenCounts, Tool, UsageEvent};
    use crate::pricing::Pricing;
    use crate::quota_store::{ClaudeSnapshot, CodexSnapshot};
    use crate::recorder::RecorderStatus;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "token-audit-share-{}-{}-{}",
                std::process::id(),
                nanos,
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn at(value: &str) -> chrono::DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn tokens(input: u64, cache_read: u64) -> TokenCounts {
        TokenCounts {
            input,
            cache_read,
            ..TokenCounts::default()
        }
    }

    fn event(
        tool: Tool,
        model: &str,
        timestamp: &str,
        project: Option<&str>,
        session: &str,
        counts: TokenCounts,
    ) -> UsageEvent {
        UsageEvent {
            tool,
            source: if tool == Tool::Claude {
                Source::ClaudeCli
            } else {
                Source::CodexCli
            },
            session_id: session.to_owned(),
            request_id: None,
            timestamp: at(timestamp),
            model: model.to_owned(),
            project: project.map(str::to_owned),
            is_subagent: false,
            tokens: counts,
        }
    }

    fn claude_plan(plan: &str) -> ClaudeSnapshot {
        ClaudeSnapshot {
            at: at("2026-10-01T00:00:00Z"),
            subscription_type: Some(plan.to_owned()),
            five_hour: None,
            seven_day: None,
            weekly_breakdown: None,
        }
    }

    fn codex_plan(plan: &str, timestamp: &str) -> CodexSnapshot {
        CodexSnapshot {
            at: at(timestamp),
            limit_id: None,
            plan_type: Some(plan.to_owned()),
            primary: None,
            secondary: None,
            reset_credits_available: None,
        }
    }

    fn codex_log(plan: &str, timestamp: &str) -> RateLimitSnapshot {
        RateLimitSnapshot {
            timestamp: at(timestamp),
            session_id: "rate-limit-row".to_owned(),
            source: Source::CodexCli,
            limit_id: None,
            plan_type: Some(plan.to_owned()),
            primary: None,
            secondary: None,
        }
    }

    fn summarize(
        events: &[UsageEvent],
        claude: &[ClaudeSnapshot],
        codex: &[CodexSnapshot],
        log: &[RateLimitSnapshot],
        now: chrono::DateTime<Utc>,
        tz: FixedOffset,
    ) -> super::ShareSummary {
        let pricing = Pricing::bundled().expect("pricing");
        let titles = BTreeMap::new();
        let attribution = attribute(events, &[], &titles, &[], claude, &pricing);
        share_summary(
            &OverviewInput {
                events,
                titles: &titles,
                points: &[],
                attribution: &attribution,
                claude_snapshots: claude,
                codex_snapshots: codex,
                codex_log: log,
                pricing: &pricing,
                recorder: RecorderStatus::default(),
            },
            now,
            tz,
        )
    }

    fn approx(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn october() -> (chrono::DateTime<Utc>, FixedOffset) {
        let tz = FixedOffset::east_opt(0).expect("utc");
        (at("2026-10-20T00:00:00Z"), tz)
    }

    fn sample_events() -> Vec<UsageEvent> {
        vec![
            event(
                Tool::Claude,
                "claude-opus-5-5",
                "2026-10-10T00:00:00Z",
                Some("/work/secret/claude-app"),
                "claude-session-private",
                tokens(1_000_000, 0),
            ),
            event(
                Tool::Codex,
                "gpt-6.1-sol",
                "2026-10-11T00:00:00Z",
                Some("/work/secret/codex-app"),
                "codex-session-private",
                tokens(1_000_000, 0),
            ),
        ]
    }

    #[test]
    fn both_recognized_plans_use_both_costs() {
        let events = sample_events();
        let claude = [claude_plan("Pro")];
        let codex = [codex_plan("plus", "2026-10-02T00:00:00Z")];
        let log = [codex_log("pro", "2026-09-01T00:00:00Z")];
        let (now, tz) = october();
        let summary = summarize(&events, &claude, &codex, &log, now, tz);
        assert_eq!(summary.year, 2026);
        assert_eq!(summary.month, 10);
        approx(summary.month_estimated_cost_usd, 6.0);
        assert_eq!(summary.recognized_subscriptions, 2);
        approx(summary.subscription_total_usd.expect("total"), 40.0);
        approx(
            summary.subscription_value_multiple.expect("multiple"),
            6.0 / 40.0,
        );
        assert!(summary.unrecognized_tools.is_empty());
        assert_eq!(summary.tools.len(), 2);
        assert_eq!(summary.tools[0].tool, Tool::Claude);
        assert_eq!(summary.tools[0].plan.as_deref(), Some("Pro"));
        approx(
            summary.tools[0].subscription_usd.expect("claude price"),
            20.0,
        );
        approx(summary.tools[0].estimated_cost_usd, 4.0);
        assert_eq!(summary.tools[1].tool, Tool::Codex);
        assert_eq!(summary.tools[1].plan.as_deref(), Some("plus"));
        approx(
            summary.tools[1].subscription_usd.expect("codex price"),
            20.0,
        );
        approx(summary.tools[1].estimated_cost_usd, 2.0);
        assert_eq!(summary.requests, 2);
        assert_eq!(summary.cache_hit_rate, Some(0.0));
        let project = summary.top_project.as_ref().expect("project");
        assert_eq!(project.name, "claude-app");
        approx(project.estimated_cost_usd, 4.0);
        let json = serde_json::to_string(&summary).expect("json");
        assert!(!json.contains("claude-session-private"));
        assert!(!json.contains("codex-session-private"));
        assert!(!json.contains("/work/secret"));
    }

    #[test]
    fn one_recognized_plan_excludes_the_other_cost() {
        let events = sample_events();
        let claude = [claude_plan("pro")];
        let log = [codex_log("free", "2026-10-02T00:00:00Z")];
        let (now, tz) = october();
        let summary = summarize(&events, &claude, &[], &log, now, tz);
        assert_eq!(summary.recognized_subscriptions, 1);
        approx(summary.subscription_total_usd.expect("total"), 20.0);
        approx(
            summary.subscription_value_multiple.expect("multiple"),
            4.0 / 20.0,
        );
        assert!((summary.subscription_value_multiple.expect("multiple") - 6.0 / 20.0).abs() > 1e-6);
        assert_eq!(summary.unrecognized_tools, vec![Tool::Codex]);
        assert!(summary.tools[1].subscription_usd.is_none());
        approx(summary.month_estimated_cost_usd, 6.0);
        approx(summary.tools[1].estimated_cost_usd, 2.0);
    }

    #[test]
    fn unrecognized_plans_have_no_multiple() {
        let events = sample_events();
        let claude = [claude_plan("team")];
        let log = [codex_log("free", "2026-10-02T00:00:00Z")];
        let (now, tz) = october();
        let summary = summarize(&events, &claude, &[], &log, now, tz);
        assert_eq!(summary.recognized_subscriptions, 0);
        assert_eq!(summary.subscription_total_usd, None);
        assert_eq!(summary.subscription_value_multiple, None);
        assert_eq!(summary.unrecognized_tools, vec![Tool::Claude, Tool::Codex]);
        assert_eq!(summary.tools[0].plan.as_deref(), Some("team"));
        assert!(summary.tools[0].subscription_usd.is_none());
        approx(summary.month_estimated_cost_usd, 6.0);
    }

    #[test]
    fn priced_plan_without_usage_still_counts_in_the_denominator() {
        let events = [event(
            Tool::Codex,
            "gpt-6.1-sol",
            "2026-10-11T00:00:00Z",
            Some("/work/codex-app"),
            "codex-only",
            tokens(1_000_000, 0),
        )];
        let claude = [claude_plan("pro")];
        let codex = [codex_plan("plus", "2026-10-02T00:00:00Z")];
        let (now, tz) = october();
        let summary = summarize(&events, &claude, &codex, &[], now, tz);
        assert_eq!(summary.recognized_subscriptions, 2);
        approx(summary.subscription_total_usd.expect("total"), 40.0);
        approx(
            summary.subscription_value_multiple.expect("multiple"),
            2.0 / 40.0,
        );
        assert_eq!(summary.tools.len(), 1);
        assert_eq!(summary.tools[0].tool, Tool::Codex);
        assert!(summary.unrecognized_tools.is_empty());
        approx(summary.month_estimated_cost_usd, 2.0);
    }

    #[test]
    fn local_month_boundary_and_project_directory_name() {
        let tz = FixedOffset::east_opt(8 * 3600).expect("offset");
        let now = tz
            .with_ymd_and_hms(2026, 10, 15, 12, 0, 0)
            .single()
            .expect("now")
            .with_timezone(&Utc);
        let events = vec![
            event(
                Tool::Claude,
                "claude-opus-5-5",
                "2026-09-30T16:00:00Z",
                Some("/home/private-root/work/alpha-proj"),
                "sess-private-9f3a",
                tokens(0, 1_000_000),
            ),
            event(
                Tool::Codex,
                "gpt-6.1-sol",
                "2026-10-31T15:59:59Z",
                Some("D:\\hidden-drive\\jobs\\beta-proj\\"),
                "sess-other",
                tokens(4_000_000, 0),
            ),
            event(
                Tool::Claude,
                "claude-opus-5-5",
                "2026-09-30T15:59:59Z",
                Some("/leak/previous-month-project"),
                "sess-previous",
                tokens(100_000_000, 50_000_000),
            ),
            event(
                Tool::Codex,
                "gpt-6.1-sol",
                "2026-10-31T16:00:00Z",
                Some("/leak/next-month-project"),
                "sess-next",
                tokens(100_000_000, 0),
            ),
        ];
        let summary = summarize(&events, &[], &[], &[], now, tz);
        assert_eq!(summary.year, 2026);
        assert_eq!(summary.month, 10);
        assert_eq!(summary.requests, 2);
        // Claude cache read is $0.20 per million; Codex input is $2 per million.
        approx(summary.month_estimated_cost_usd, 0.2 + 8.0);
        approx(
            summary.cache_hit_rate.expect("hit"),
            1_000_000.0 / 5_000_000.0,
        );
        let project = summary.top_project.as_ref().expect("project");
        assert_eq!(project.name, "beta-proj");
        approx(project.estimated_cost_usd, 8.0);
        let json = serde_json::to_value(&summary).expect("json");
        let text = json.to_string();
        assert!(!text.contains("private-root"));
        assert!(!text.contains("hidden-drive"));
        assert!(!text.contains("previous-month-project"));
        assert!(!text.contains("next-month-project"));
        assert!(!text.contains("sess-private-9f3a"));
        assert!(!text.contains("sess-previous"));
        assert_eq!(json["top_project"]["name"], json!("beta-proj"));
        assert!(json.get("session_id").is_none());
        assert!(json.get("title").is_none());
    }

    #[test]
    fn save_export_file_limits_names_and_avoids_collisions() {
        let temp = Temp::new();
        let first = save_export_file(&temp.0, "token-audit-2026-10.png", b"one").expect("first");
        assert_eq!(fs::read(&first).expect("read"), b"one");
        let second = save_export_file(&temp.0, "token-audit-2026-10.png", b"two").expect("second");
        let third = save_export_file(&temp.0, "token-audit-2026-10.png", b"three").expect("third");
        assert!(first.ends_with("token-audit-2026-10.png"));
        assert!(second.ends_with("token-audit-2026-10-2.png"));
        assert!(third.ends_with("token-audit-2026-10-3.png"));
        assert_eq!(fs::read(&first).expect("original"), b"one");
        assert_eq!(fs::read(&second).expect("second read"), b"two");
        let markdown = save_export_file(&temp.0, "note.md", b"# hi").expect("md");
        assert!(markdown.ends_with("note.md"));
        assert_eq!(
            save_export_file(&temp.0, "a/b.png", b"x").expect_err("slash"),
            "文件名无效"
        );
        assert_eq!(
            save_export_file(&temp.0, "a\\b.png", b"x").expect_err("backslash"),
            "文件名无效"
        );
        assert_eq!(
            save_export_file(&temp.0, "../secret.png", b"x").expect_err("parent"),
            "文件名无效"
        );
        assert_eq!(
            save_export_file(&temp.0, "note.txt", b"x").expect_err("txt"),
            "只能导出 PNG 或 Markdown 文件"
        );
        assert_eq!(
            save_export_file(&temp.0, ".png", b"x").expect_err("dot"),
            "文件名无效"
        );
    }
}
