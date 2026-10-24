use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use serde_json::json;
use token_audit_core::advice::{build_advice, AdviceConfig, AdviceInput};
use token_audit_core::attribution::{attribute, WindowKind};
use token_audit_core::claude::scan_claude;
use token_audit_core::codex::scan_codex;
use token_audit_core::dashboard::{build_overview, OverviewInput, QuotaDisplay};
use token_audit_core::model::Tool;
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{read_claude, read_codex};
use token_audit_core::recorder::{read_statusline_records, status};
use token_audit_core::sessions::{list_sessions, session_detail, SessionInput};
use token_audit_core::settings::read_settings;
use token_audit_core::timeline::build_timeline;
use token_audit_core::usage::{list_usage_windows, usage_detail};

struct Args {
    home: PathBuf,
    data_dir: PathBuf,
    tool: Option<Tool>,
    json: bool,
    window: Option<String>,
    days: u32,
    id: Option<String>,
}
fn quota_label(display: &QuotaDisplay) -> String {
    if display.window_passed {
        "已重置，等待新数据".to_owned()
    } else {
        display
            .used_percent
            .map_or_else(|| "暂无数据".to_owned(), |percent| format!("{percent}%"))
    }
}
fn parse(args: impl Iterator<Item = OsString>, command: &str) -> Result<Args, String> {
    let mut home = None;
    let mut data_dir = None;
    let mut tool = None;
    let mut json = false;
    let mut window = None;
    let mut days = None;
    let mut id = None;
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or("参数名不是有效文本")?;
        match flag {
            "--json" if !json => json = true,
            "--home" if home.is_none() => {
                home = Some(PathBuf::from(args.next().ok_or("--home 缺少目录")?))
            }
            "--data-dir" if data_dir.is_none() => {
                data_dir = Some(PathBuf::from(args.next().ok_or("--data-dir 缺少目录")?))
            }
            "--tool" if !matches!(command, "overview" | "sessions") && tool.is_none() => {
                tool = Some(match args.next().as_deref().and_then(|s| s.to_str()) {
                    Some("claude") => Tool::Claude,
                    Some("codex") => Tool::Codex,
                    _ => return Err("--tool 只能是 claude 或 codex".into()),
                });
            }
            "--window" if command == "usage" && window.is_none() => {
                window = Some(
                    args.next()
                        .ok_or("--window 缺少窗口 ID")?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            "--id" if command == "session" && id.is_none() => {
                let value = args.next().ok_or("--id 缺少会话 ID")?;
                if value.to_str().is_some_and(|text| text.starts_with("--")) {
                    return Err("--id 缺少会话 ID".to_owned());
                }
                id = Some(value.to_string_lossy().into_owned());
            }
            "--days" if matches!(command, "advice" | "sessions") && days.is_none() => {
                days = Some(match args.next().as_deref().and_then(|s| s.to_str()) {
                    Some("7") => 7,
                    Some("30") => 30,
                    _ => return Err("--days 只能是 7 或 30".into()),
                });
            }
            _ => return Err(format!("未知或重复的参数：{flag}")),
        }
    }
    Ok(Args {
        home: home.or_else(dirs::home_dir).ok_or("无法确定用户主目录")?,
        data_dir: data_dir
            .or_else(|| dirs::data_dir().map(|p| p.join("token-audit")))
            .ok_or("无法确定数据目录")?,
        tool,
        json,
        window,
        days: days.unwrap_or(7),
        id,
    })
}

pub fn run(command: &str, args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = parse(args, command)?;
    let claude = scan_claude(&args.home).map_err(|_| "扫描 Claude 日志失败")?;
    let codex = scan_codex(&args.home).map_err(|_| "扫描 Codex 日志失败")?;
    let (claude_snapshots, _) = read_claude(&args.data_dir);
    let (codex_snapshots, _) = read_codex(&args.data_dir);
    let (statusline, _) = read_statusline_records(&args.data_dir);
    let settings = read_settings(&args.data_dir).map_err(|_| "读取应用设置失败")?;
    let points = build_timeline(
        &claude_snapshots,
        &codex_snapshots,
        &statusline,
        &codex.rate_limits,
        settings.calibration.as_ref(),
    );
    let pricing = Pricing::bundled().map_err(str::to_owned)?;
    let mut events = claude.events;
    events.extend(codex.events);
    let mut prompts = claude.prompts;
    prompts.extend(codex.prompts);
    let mut titles: BTreeMap<_, _> = claude.titles;
    titles.extend(codex.titles);
    let attribution = attribute(
        &events,
        &prompts,
        &titles,
        &points,
        &claude_snapshots,
        &pricing,
    );
    let session_input = SessionInput {
        events: &attribution.events,
        titles: &titles,
        pricing: &pricing,
    };
    if command == "sessions" {
        let rows = list_sessions(&session_input, args.days, Utc::now());
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&rows).map_err(|_| "生成 JSON 失败")?
            );
        } else {
            println!("近 {} 天：{} 个会话", args.days, rows.len());
            for row in rows.iter().take(30) {
                let hit = row
                    .cache_hit_rate
                    .map_or_else(|| "—".to_owned(), |rate| format!("{:.0}%", rate * 100.0));
                let quota = if row.has_quota_data {
                    format!("{:.1}%", row.weekly_delta)
                } else {
                    "—".to_owned()
                };
                println!(
                    "{:?} {}  {}  请求 {}  token {}  命中 {}  额度 {}  {}",
                    row.tool,
                    row.session_id,
                    row.display_name,
                    row.requests,
                    row.total_tokens,
                    hit,
                    quota,
                    row.model
                );
            }
            if rows.len() > 30 {
                println!("其余 {} 个会话请用 --json 查看", rows.len() - 30);
            }
        }
        return Ok(());
    } else if command == "session" {
        let tool = args.tool.ok_or("session 命令需要 --tool claude|codex")?;
        let id = args.id.ok_or("session 命令需要 --id")?;
        let config = AdviceConfig::bundled().map_err(str::to_owned)?;
        let report = build_advice(
            &AdviceInput {
                events: &attribution.events,
                prompts: &prompts,
                pricing: &pricing,
                config: &config,
                dismissed: &settings.dismissed_advice,
            },
            30,
            Utc::now(),
        );
        let detail = session_detail(&session_input, &prompts, &report.event_tags, tool, &id)
            .ok_or("未找到该会话")?;
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&detail).map_err(|_| "生成 JSON 失败")?
            );
        } else {
            println!("{}（{}）", detail.display_name, detail.session_id);
            println!(
                "{} · {:?} · {}",
                if detail.project_name.is_empty() {
                    "未知项目"
                } else {
                    detail.project_name.as_str()
                },
                detail.source,
                detail.model
            );
            println!(
                "请求 {}，token {}，每周归因 {:.1}",
                detail.requests, detail.total_tokens, detail.weekly_delta
            );
            println!("请求明细请用 --json 查看");
        }
        return Ok(());
    } else if command == "advice" {
        let config = AdviceConfig::bundled().map_err(str::to_owned)?;
        let report = build_advice(
            &AdviceInput {
                events: &attribution.events,
                prompts: &prompts,
                pricing: &pricing,
                config: &config,
                dismissed: &settings.dismissed_advice,
            },
            args.days,
            Utc::now(),
        );
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|_| "生成 JSON 失败")?
            );
        } else {
            for tool in &report.tools {
                println!(
                    "{:?}：{} 项，可避免 {}，涉及 {} 个会话（已隐藏 {} 项）",
                    tool.tool,
                    tool.summary.count,
                    tool.summary.avoidable_weekly_pct.map_or_else(
                        || format!("约 ${:.2}", tool.summary.avoidable_cost_usd),
                        |pct| format!("{pct:.1}% 周额度")
                    ),
                    tool.summary.sessions,
                    tool.hidden
                );
                for item in &tool.items {
                    println!(
                        "  {} [{:?}] 消耗 {} · 可避免 {} · {}",
                        item.title,
                        item.severity,
                        item.consumed_weekly_pct.map_or_else(
                            || format!("${:.2}", item.overhead_cost_usd),
                            |pct| format!("{pct:.1}%")
                        ),
                        item.avoidable_weekly_pct.map_or_else(
                            || format!("${:.2}", item.avoidable_cost_usd),
                            |pct| format!("{pct:.1}%")
                        ),
                        item.facts.join(" / ")
                    );
                }
            }
        }
    } else if command == "usage" {
        let now = Utc::now();
        let input = OverviewInput {
            events: &events,
            titles: &titles,
            points: &points,
            attribution: &attribution,
            claude_snapshots: &claude_snapshots,
            codex_snapshots: &codex_snapshots,
            codex_log: &codex.rate_limits,
            pricing: &pricing,
            recorder: status(&args.home, &args.data_dir),
        };
        let tool = args.tool.unwrap_or_else(|| {
            if list_usage_windows(&input, Tool::Claude, now, 1).is_empty() {
                Tool::Codex
            } else {
                Tool::Claude
            }
        });
        let windows = list_usage_windows(&input, tool, now, 6);
        let all_windows;
        let search_windows = if args.window.as_deref().is_some_and(|w| w != "current") {
            all_windows = list_usage_windows(&input, tool, now, usize::MAX);
            &all_windows
        } else {
            &windows
        };
        let selected = match args.window.as_deref() {
            Some("current") => windows
                .iter()
                .find(|w| w.kind == WindowKind::FiveHour && w.is_current),
            Some(id) => {
                let id = DateTime::parse_from_rfc3339(id)
                    .map_err(|_| "--window 必须是 current 或 RFC3339 窗口 ID")?
                    .with_timezone(&Utc);
                search_windows.iter().find(|w| w.window_id == id)
            }
            None => windows
                .iter()
                .find(|w| w.kind == WindowKind::FiveHour && w.is_current)
                .or_else(|| windows.first()),
        };
        if args.window.is_some() && selected.is_none() {
            return Err("指定的用量窗口不存在".to_owned());
        }
        let detail = selected.map(|w| usage_detail(&input, w, now));
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "windows": windows, "detail": detail }))
                    .map_err(|_| "生成 JSON 失败")?
            );
        } else if let Some(detail) = detail {
            println!(
                "{:?} {:?} {}：已用 {:.1}%，已归属 {:.1}%，未归属 {:.1}%",
                tool,
                detail.window.kind,
                detail.window.window_id,
                detail.used_percent,
                detail.attributed_total,
                detail.unattributed_total
            );
            for row in detail.sessions.iter().take(8) {
                println!("{}：{} token，{:.1}%", row.name, row.tokens, row.delta);
            }
        } else {
            println!("暂无额度数据");
        }
    } else if command == "overview" {
        let overview = build_overview(
            &OverviewInput {
                events: &events,
                titles: &titles,
                points: &points,
                attribution: &attribution,
                claude_snapshots: &claude_snapshots,
                codex_snapshots: &codex_snapshots,
                codex_log: &codex.rate_limits,
                pricing: &pricing,
                recorder: status(&args.home, &args.data_dir),
            },
            Utc::now(),
            Local,
        );
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&overview).map_err(|_| "生成 JSON 失败")?
            );
        } else {
            println!(
                "最近 7 天：{} 个会话，{} 次请求，{} 条高耗提示",
                overview.summary.sessions, overview.summary.requests, overview.summary.high_prompts
            );
            for tool in overview.tools {
                println!(
                    "{:?}：5 小时 {}，每周 {}",
                    tool.tool,
                    quota_label(&tool.five_hour),
                    quota_label(&tool.weekly)
                );
            }
            println!(
                "今日 token：{}；本月 API 等价估算：${:.4}",
                overview.today_tokens.total_without_reasoning(),
                overview.month_estimated_cost_usd
            );
        }
    } else {
        let windows: Vec<_> = attribution
            .windows
            .iter()
            .filter(|w| args.tool.is_none_or(|t| t == w.tool))
            .collect();
        let mut sessions = Vec::new();
        for window in &windows {
            for row in &window.by_session {
                sessions.push((
                    window.tool,
                    window.kind,
                    window.window_id,
                    row.key.clone(),
                    row.delta,
                ));
            }
        }
        sessions.sort_by(|a, b| b.4.total_cmp(&a.4).then_with(|| a.3.cmp(&b.3)));
        sessions.truncate(10);
        let mut high_prompts: Vec<_> = attribution
            .window_prompts
            .iter()
            .filter(|p| {
                p.kind == WindowKind::FiveHour
                    && args.tool.is_none_or(|t| t == p.tool)
                    && p.prompt_at.is_some()
            })
            .collect();
        high_prompts.sort_by(|a, b| {
            b.delta
                .total_cmp(&a.delta)
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        high_prompts.truncate(10);
        if args.json {
            let summary = json!({ "windows": windows, "top_sessions": sessions.iter().map(|(tool, kind, id, session, delta)| json!({"tool":tool,"kind":kind,"window_id":id,"session_id_prefix":session.chars().take(8).collect::<String>(),"title":titles.get(session),"delta":delta})).collect::<Vec<_>>(), "top_prompts": high_prompts });
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).map_err(|_| "生成 JSON 失败")?
            );
        } else {
            for w in windows {
                println!(
                    "{:?} {:?} {}：{} 点，增量 {:.3}，已归属 {:.3}，未归属 {:.3}，下降 {}",
                    w.tool,
                    w.kind,
                    w.window_id,
                    w.points,
                    w.increment_total,
                    w.attributed_total,
                    w.unattributed_total,
                    w.decreases
                );
            }
            println!("\n会话前 10：");
            for (tool, kind, _, session, delta) in sessions {
                println!(
                    "{:?} {:?} {} {} {:.3}",
                    tool,
                    kind,
                    session.chars().take(8).collect::<String>(),
                    titles.get(&session).map(String::as_str).unwrap_or(""),
                    delta
                );
            }
            println!("\n高耗提示前 10：");
            for p in high_prompts {
                println!(
                    "{:?} {} {} {:.3}",
                    p.tool,
                    p.session_id.chars().take(8).collect::<String>(),
                    p.title.as_deref().unwrap_or(""),
                    p.delta
                );
            }
        }
    }
    Ok(())
}
