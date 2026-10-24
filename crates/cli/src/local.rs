use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use chrono::{Local, Utc};
use serde_json::json;
use token_audit_core::attribution::{attribute, WindowKind};
use token_audit_core::claude::scan_claude;
use token_audit_core::codex::scan_codex;
use token_audit_core::dashboard::{build_overview, OverviewInput, QuotaDisplay};
use token_audit_core::model::Tool;
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{read_claude, read_codex};
use token_audit_core::recorder::{read_statusline_records, status};
use token_audit_core::settings::read_settings;
use token_audit_core::timeline::build_timeline;

struct Args {
    home: PathBuf,
    data_dir: PathBuf,
    tool: Option<Tool>,
    json: bool,
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
fn parse(args: impl Iterator<Item = OsString>, attribution: bool) -> Result<Args, String> {
    let mut home = None;
    let mut data_dir = None;
    let mut tool = None;
    let mut json = false;
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
            "--tool" if attribution && tool.is_none() => {
                tool = Some(match args.next().as_deref().and_then(|s| s.to_str()) {
                    Some("claude") => Tool::Claude,
                    Some("codex") => Tool::Codex,
                    _ => return Err("--tool 只能是 claude 或 codex".into()),
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
    })
}

pub fn run(command: &str, args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = parse(args, command == "attribution")?;
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
    if command == "overview" {
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
