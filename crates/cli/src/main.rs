mod claude_quota;
mod local;
mod recorder;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{Days, Local, NaiveDate};
use token_audit_core::claude::{scan_claude, ClaudeDebugSummary};
use token_audit_core::codex::{scan_codex, CodexDebugSummary, RateLimitWindow};
use token_audit_core::codex_app_server::{
    codex_candidates, read_codex_quota, resolve_launch, AppServerOptions, CodexQuota,
};
use token_audit_core::model::{Source, Tool};
use token_audit_core::pricing::Pricing;
use token_audit_core::report::{build_report, Metrics, Report, ReportOptions};

fn usage() {
    println!("       token-audit attribution [--home <目录>] [--data-dir <目录>] [--tool claude|codex] [--json]\n       token-audit overview [--home <目录>] [--data-dir <目录>] [--json]\n       token-audit usage [--home <目录>] [--data-dir <目录>] [--tool claude|codex] [--window current|<RFC3339 window_id>] [--json]");
    println!(
        "用法：token-audit debug <claude|codex> [--home <目录>] [--json]\n       token-audit scan [--home <目录>] [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--tool claude|codex] [--pricing <覆盖文件>] [--json]\n       token-audit quota codex [--codex <路径>] [--timeout <秒>] [--json]\n       token-audit quota claude [--source auto|desktop|code] [--proxy <url>] [--no-proxy] [--timeout <秒>] [--keychain-timeout <秒>] [--json]\n       token-audit recorder <status|install|uninstall|records> [选项]\n       token-audit hook <statusline|event --tool claude>\n       token-audit --version"
    );
}

fn parse_debug_args(args: impl Iterator<Item = OsString>) -> Result<(PathBuf, bool), &'static str> {
    let mut home = None;
    let mut json = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--home") => {
                if home.is_some() {
                    return Err("--home 只能指定一次");
                }
                let value = args.next().ok_or("--home 需要目录参数")?;
                if value.to_str().is_some_and(|value| value.starts_with("--")) {
                    return Err("--home 需要目录参数");
                }
                home = Some(PathBuf::from(value));
            }
            Some("--json") => json = true,
            _ => return Err("未知参数"),
        }
    }
    let home = home
        .or_else(dirs::home_dir)
        .ok_or("无法确定用户主目录，请指定 --home")?;
    Ok((home, json))
}

fn print_claude_summary(summary: &ClaudeDebugSummary) {
    let stats = &summary.stats;
    println!("文件数：{}", stats.files);
    println!("总行数：{}", stats.total_lines);
    println!("有效用量行数：{}", stats.valid_usage_lines);
    println!("去重丢弃数：{}", stats.deduplicated_lines);
    println!("格式错误行数：{}", stats.malformed_lines);
    println!("非用量行数：{}", stats.non_usage_lines);
    println!("读取失败文件数：{}", stats.unreadable_files);
    println!("事件总数：{}", summary.events_total);
    println!("唯一 session 数：{}", summary.unique_sessions);
    println!("input：{}", summary.tokens.input);
    println!("output：{}", summary.tokens.output);
    println!("reasoning：{}", summary.tokens.reasoning);
    println!("cache_read：{}", summary.tokens.cache_read);
    println!("cache_write_5m：{}", summary.tokens.cache_write_5m);
    println!("cache_write_1h：{}", summary.tokens.cache_write_1h);
    println!(
        "cache_write_unspecified：{}",
        summary.tokens.cache_write_unspecified
    );
    println!("ClaudeCli 事件数：{}", summary.sources.claude_cli);
    println!("ClaudeDesktop 事件数：{}", summary.sources.claude_desktop);
    println!("Unknown 事件数：{}", summary.sources.unknown);
    for (model, counts) in &summary.models {
        println!(
            "模型 {model}：事件数 {}，output {}",
            counts.events, counts.output
        );
    }
}

fn print_window(name: &str, window: Option<&RateLimitWindow>) {
    if let Some(window) = window {
        println!("{name} used_percent：{}", window.used_percent);
        println!("{name} window_minutes：{:?}", window.window_minutes);
        println!("{name} resets_at：{:?}", window.resets_at);
    } else {
        println!("{name}：无");
    }
}

fn print_codex_summary(summary: &CodexDebugSummary) {
    let stats = &summary.stats;
    println!("文件数：{}", stats.files);
    println!("总行数：{}", stats.total_lines);
    println!("格式错误行数：{}", stats.malformed_lines);
    println!("非用量行数：{}", stats.non_usage_lines);
    println!("record 行数：{}", stats.usage_records);
    println!("旧版用量行数：{}", stats.token_count_usage_lines);
    println!("去重丢弃数：{}", stats.deduplicated_lines);
    println!("含 record 文件数：{}", stats.files_with_records);
    println!("仅 token_count 文件数：{}", stats.files_token_count_only);
    println!("额度快照数：{}", stats.rate_limit_snapshots);
    println!("读取失败文件数：{}", stats.unreadable_files);
    println!("事件总数：{}", summary.events_total);
    println!("唯一 session 数：{}", summary.unique_sessions);
    println!("input：{}", summary.tokens.input);
    println!("output：{}", summary.tokens.output);
    println!("reasoning：{}", summary.tokens.reasoning);
    println!("cache_read：{}", summary.tokens.cache_read);
    println!("cache_write_5m：{}", summary.tokens.cache_write_5m);
    println!("cache_write_1h：{}", summary.tokens.cache_write_1h);
    println!(
        "cache_write_unspecified：{}",
        summary.tokens.cache_write_unspecified
    );
    println!("CodexCli 事件数：{}", summary.sources.codex_cli);
    println!("CodexDesktop 事件数：{}", summary.sources.codex_desktop);
    println!("Unknown 事件数：{}", summary.sources.unknown);
    for (model, counts) in &summary.models {
        println!(
            "模型 {model}：事件数 {}，output {}",
            counts.events, counts.output
        );
    }
    println!("额度快照总数：{}", summary.rate_limit_snapshots);
    if let Some(latest) = &summary.latest_rate_limit {
        println!("最新额度 limit_id：{:?}", latest.limit_id);
        println!("最新额度 plan_type：{:?}", latest.plan_type);
        print_window("primary", latest.primary.as_ref());
        print_window("secondary", latest.secondary.as_ref());
    } else {
        println!("最新额度：无");
    }
}

fn run_debug(tool: &str, args: impl Iterator<Item = OsString>) -> Result<(), &'static str> {
    let (home, json) = parse_debug_args(args)?;
    match tool {
        "claude" => {
            let summary = scan_claude(&home)
                .map_err(|_| "扫描 Claude 日志失败")?
                .debug_summary();
            if json {
                let output =
                    serde_json::to_string_pretty(&summary).map_err(|_| "生成 JSON 失败")?;
                println!("{output}");
            } else {
                print_claude_summary(&summary);
            }
        }
        "codex" => {
            let summary = scan_codex(&home)
                .map_err(|_| "扫描 Codex 日志失败")?
                .debug_summary();
            if json {
                let output =
                    serde_json::to_string_pretty(&summary).map_err(|_| "生成 JSON 失败")?;
                println!("{output}");
            } else {
                print_codex_summary(&summary);
            }
        }
        _ => return Err("未知调试工具"),
    }
    Ok(())
}

struct ScanArgs {
    home: PathBuf,
    since: Option<NaiveDate>,
    until: Option<NaiveDate>,
    tool: Option<Tool>,
    pricing: Option<PathBuf>,
    json: bool,
}

fn parse_scan_args(args: impl Iterator<Item = OsString>) -> Result<ScanArgs, String> {
    let mut home = None;
    let mut since = None;
    let mut until = None;
    let mut tool = None;
    let mut pricing = None;
    let mut json = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let flag = arg.to_str().ok_or("参数名不是有效文本")?;
        match flag {
            "--json" if !json => json = true,
            "--home" | "--since" | "--until" | "--tool" | "--pricing" => {
                let value = args.next().ok_or_else(|| format!("{flag} 缺少参数"))?;
                if value.to_str().is_some_and(|text| text.starts_with("--")) {
                    return Err(format!("{flag} 缺少参数"));
                }
                match flag {
                    "--home" if home.is_none() => home = Some(PathBuf::from(value)),
                    "--pricing" if pricing.is_none() => pricing = Some(PathBuf::from(value)),
                    "--since" if since.is_none() => {
                        since = Some(parse_date(&value, flag)?);
                    }
                    "--until" if until.is_none() => {
                        until = Some(parse_date(&value, flag)?);
                    }
                    "--tool" if tool.is_none() => {
                        tool = Some(match value.to_str() {
                            Some("claude") => Tool::Claude,
                            Some("codex") => Tool::Codex,
                            _ => return Err("--tool 只能是 claude 或 codex".into()),
                        });
                    }
                    _ => return Err(format!("{flag} 只能指定一次")),
                }
            }
            _ => return Err(format!("未知参数：{flag}")),
        }
    }
    if since.zip(until).is_some_and(|(start, end)| start > end) {
        return Err("--since 不能晚于 --until".into());
    }
    Ok(ScanArgs {
        home: home
            .or_else(dirs::home_dir)
            .ok_or("无法确定用户主目录，请指定 --home")?,
        since,
        until,
        tool,
        pricing,
        json,
    })
}

fn parse_date(value: &OsString, flag: &str) -> Result<NaiveDate, String> {
    let value = value
        .to_str()
        .ok_or_else(|| format!("{flag} 日期不是有效文本"))?;
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("{flag} 日期格式应为 YYYY-MM-DD"))
}

fn load_pricing(explicit: Option<PathBuf>) -> Result<Pricing, String> {
    let mut pricing = Pricing::bundled().map_err(str::to_owned)?;
    let is_explicit = explicit.is_some();
    let path = explicit
        .or_else(|| dirs::config_dir().map(|directory| directory.join("token-audit/pricing.toml")));
    if let Some(path) = path {
        match fs::read_to_string(&path) {
            Ok(content) => pricing = pricing.with_override(&content).map_err(str::to_owned)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !is_explicit => {}
            Err(error) => return Err(format!("无法读取价格覆盖文件 {}：{error}", path.display())),
        }
    }
    Ok(pricing)
}

const NAME_WIDTH: usize = 28;

fn char_display_width(ch: char) -> usize {
    match ch as u32 {
        0x1100..=0x115f
        | 0x2329..=0x232a
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe19
        | 0xfe30..=0xfe6f
        | 0xff01..=0xff60
        | 0xffe0..=0xffe6
        | 0x20000..=0x3fffd => 2,
        _ => 1,
    }
}

fn format_name(label: &str) -> String {
    let full_width: usize = label.chars().map(char_display_width).sum();
    if full_width <= NAME_WIDTH {
        return format!("{label}{}", " ".repeat(NAME_WIDTH - full_width));
    }

    let mut result = String::new();
    let mut width = 0;
    for ch in label.chars() {
        let char_width = char_display_width(ch);
        if width + char_width > NAME_WIDTH - 1 {
            break;
        }
        result.push(ch);
        width += char_width;
    }
    result.push('…');
    result.push_str(&" ".repeat(NAME_WIDTH - width - 1));
    result
}

fn print_row(label: &str, metrics: &Metrics) {
    let writes = metrics.tokens.cache_write_5m as u128
        + metrics.tokens.cache_write_1h as u128
        + metrics.tokens.cache_write_unspecified as u128;
    let hit = metrics
        .cache_hit_rate
        .map_or_else(|| "—".to_owned(), |rate| format!("{:.1}%", rate * 100.0));
    let cost = if metrics.unpriced_events > 0 && metrics.unpriced_events == metrics.events {
        format!("—（未定价 {}）", metrics.unpriced_events)
    } else if metrics.unpriced_events > 0 {
        format!(
            "${:.4}（部分，未定价 {}）",
            metrics.estimated_cost_usd, metrics.unpriced_events
        )
    } else {
        format!("${:.4}", metrics.estimated_cost_usd)
    };
    println!(
        "{} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8} {:>16}",
        format_name(label),
        metrics.events,
        metrics.tokens.input,
        metrics.tokens.output,
        metrics.tokens.cache_read,
        writes,
        hit,
        cost
    );
}

fn print_header(title: &str) {
    println!("\n{title}");
    println!(
        "{} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8} {:>16}",
        format_name("名称"),
        "事件数",
        "输入",
        "输出",
        "缓存读",
        "缓存写",
        "命中率",
        "估算费用"
    );
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::ClaudeCli => "Claude 命令行",
        Source::ClaudeDesktop => "Claude 桌面版",
        Source::CodexCli => "Codex 命令行",
        Source::CodexDesktop => "Codex 桌面版",
        Source::Unknown => "未知来源",
    }
}

fn print_report(report: &Report, malformed: u64, unreadable: u64) {
    print_header("总计");
    print_row("总计", &report.total);
    print_header("按天");
    for group in &report.by_day {
        print_row(&group.key.to_string(), &group.metrics);
    }
    print_header("按来源");
    for group in &report.by_source {
        print_row(source_label(group.key), &group.metrics);
    }
    print_header("按模型");
    for group in &report.by_model {
        print_row(&group.key, &group.metrics);
    }
    print_header("按项目（前 10）");
    for group in report.by_project.iter().take(10) {
        let label = group
            .key
            .as_deref()
            .and_then(|path| {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
            })
            .unwrap_or("未知项目");
        print_row(label, &group.metrics);
    }
    print_header("按会话（前 10）");
    for group in report.by_session.iter().take(10) {
        let label: String = group.key.chars().take(8).collect();
        print_row(&label, &group.metrics);
    }
    println!("\n费用为按 API 价格的估算（价格表版本 {}），不是实际账单；不含 ChatGPT / Claude 桌面端聊天", report.pricing_version);
    if malformed > 0 || unreadable > 0 {
        println!("解析提示：格式错误 {malformed} 行，读取失败 {unreadable} 个文件");
    }
}

fn run_scan(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = parse_scan_args(args)?;
    let pricing = load_pricing(args.pricing.clone())?;
    let claude = scan_claude(&args.home).map_err(|_| "扫描 Claude 日志失败".to_owned())?;
    let codex = scan_codex(&args.home).map_err(|_| "扫描 Codex 日志失败".to_owned())?;
    let malformed = claude.stats.malformed_lines + codex.stats.malformed_lines;
    let unreadable = claude.stats.unreadable_files + codex.stats.unreadable_files;
    let mut events = claude.events;
    events.extend(codex.events);
    let report = build_report(
        &events,
        &pricing,
        &ReportOptions {
            since: args.since,
            until: args.until,
            tool: args.tool,
            tz: Local,
        },
    );
    if args.json {
        let output = serde_json::to_string_pretty(&report).map_err(|_| "生成 JSON 失败")?;
        println!("{output}");
    } else {
        print_report(&report, malformed, unreadable);
    }
    Ok(())
}

struct QuotaArgs {
    codex: Option<PathBuf>,
    timeout: Duration,
    json: bool,
}

fn parse_quota_args(args: impl Iterator<Item = OsString>) -> Result<QuotaArgs, String> {
    let mut codex = None;
    let mut timeout = None;
    let mut json = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let flag = arg.to_str().ok_or("参数名不是有效文本")?;
        match flag {
            "--json" if !json => json = true,
            "--codex" | "--timeout" => {
                let value = args.next().ok_or_else(|| format!("{flag} 缺少参数"))?;
                if value.to_str().is_some_and(|text| text.starts_with("--")) {
                    return Err(format!("{flag} 缺少参数"));
                }
                match flag {
                    "--codex" if codex.is_none() => codex = Some(PathBuf::from(value)),
                    "--timeout" if timeout.is_none() => {
                        let seconds = value
                            .to_str()
                            .and_then(|text| text.parse::<u64>().ok())
                            .filter(|seconds| *seconds > 0)
                            .ok_or("--timeout 应为大于 0 的整数秒")?;
                        timeout = Some(Duration::from_secs(seconds));
                    }
                    _ => return Err(format!("{flag} 只能指定一次")),
                }
            }
            _ => return Err(format!("未知参数：{flag}")),
        }
    }
    Ok(QuotaArgs {
        codex,
        timeout: timeout.unwrap_or(Duration::from_secs(15)),
        json,
    })
}

fn local_time(time: Option<chrono::DateTime<chrono::Utc>>) -> String {
    time.map(|time| {
        time.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    })
    .unwrap_or_else(|| "未知".to_owned())
}

fn print_quota_window(label: &str, window: Option<&RateLimitWindow>) {
    if let Some(window) = window {
        println!(
            "{label:<12} {:>6.1}%    重置：{}",
            window.used_percent,
            local_time(window.resets_at)
        );
    } else {
        println!("{label:<12} 未提供");
    }
}

fn print_codex_quota(quota: &CodexQuota) {
    println!("Codex 官方额度");
    println!(
        "套餐          {}",
        quota.plan_type.as_deref().unwrap_or("未知")
    );
    print_quota_window("5 小时已用", quota.primary.as_ref());
    print_quota_window("每周已用", quota.secondary.as_ref());
    let reached = quota.ordinary_usage_allowed == Some(false)
        || quota.rate_limit_reached_type.is_some()
        || quota
            .primary
            .as_ref()
            .is_some_and(|window| window.used_percent >= 100.0)
        || quota
            .secondary
            .as_ref()
            .is_some_and(|window| window.used_percent >= 100.0);
    let reached_text = if reached {
        "是"
    } else if quota.ordinary_usage_allowed == Some(true) {
        "否"
    } else {
        "未知"
    };
    println!("是否触顶      {reached_text}");
    if let Some(cards) = &quota.reset_credits {
        println!(
            "可用重置卡    {}",
            cards
                .available_count
                .map_or_else(|| "未知".to_owned(), |count| count.to_string())
        );
        for card in &cards.credits {
            if card.status.as_deref() == Some("available") {
                println!(
                    "  {}：到期 {}",
                    card.title.as_deref().unwrap_or("重置卡"),
                    local_time(card.expires_at)
                );
            }
        }
    } else {
        println!("可用重置卡    未提供");
    }
    if let Some(usage) = &quota.usage {
        let today = quota.fetched_at.with_timezone(&Local).date_naive();
        let start = today.checked_sub_days(Days::new(6));
        let tokens: u64 = usage
            .daily
            .iter()
            .filter(|day| start.is_some_and(|start| day.date >= start && day.date <= today))
            .fold(0_u64, |total, day| total.saturating_add(day.tokens));
        println!("最近 7 天官方 token 数    {tokens}");
    } else {
        println!("最近 7 天官方 token 数    未提供");
    }
    println!("\n数据来自 Codex 官方本地接口（codex app-server），由 Codex 用你已登录的账号向 OpenAI 查询；token-audit 不读取你的令牌");
}

fn run_quota(args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let args = parse_quota_args(args)?;
    let candidate = match args.codex {
        Some(candidate) if candidate.is_file() => candidate,
        Some(_) => return Err("指定的 Codex 可执行文件不存在".to_owned()),
        None => {
            let home = dirs::home_dir().ok_or("无法确定用户主目录")?;
            codex_candidates(&home, std::env::var_os("PATH").as_deref())
                .into_iter()
                .next()
                .ok_or("未找到 Codex；请先安装 Codex CLI，或用 --codex 指定可执行文件")?
        }
    };
    let launch = resolve_launch(&candidate);
    let quota = read_codex_quota(
        &launch,
        &AppServerOptions {
            timeout: args.timeout,
        },
    )
    .map_err(|error| error.to_string())?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&quota).map_err(|_| "生成 JSON 失败")?
        );
    } else {
        print_codex_quota(&quota);
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|arg| arg.to_str()) {
        Some("--version" | "-V") if args.next().is_none() => {
            println!("token-audit {}", token_audit_core::version());
            ExitCode::SUCCESS
        }
        None | Some("--help" | "-h") => {
            usage();
            ExitCode::SUCCESS
        }
        Some("hook") => recorder::run("hook", args),
        Some("recorder") => recorder::run("recorder", args),
        Some("debug") => {
            let tool = args.next();
            let tool = tool.as_deref().and_then(|arg| arg.to_str());
            let Some(tool @ ("claude" | "codex")) = tool else {
                eprintln!("未知调试工具。");
                usage();
                return ExitCode::FAILURE;
            };
            match run_debug(tool, args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    usage();
                    ExitCode::FAILURE
                }
            }
        }
        Some("scan") => match run_scan(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                usage();
                ExitCode::FAILURE
            }
        },
        Some(command @ ("attribution" | "overview" | "usage")) => match local::run(command, args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Some("quota") => {
            let result = match args.next().as_deref().and_then(|arg| arg.to_str()) {
                Some("codex") => run_quota(args),
                Some("claude") => claude_quota::run(args),
                _ => Err("未知额度来源".to_owned()),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("未知参数。");
            usage();
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{char_display_width, format_name, NAME_WIDTH};

    #[test]
    fn name_column_truncates_and_pads_by_display_width() {
        let long = format_name("这是一个很长的中文项目名称用于测试截断效果");
        assert!(long.contains('…'));
        assert_eq!(
            long.chars().map(char_display_width).sum::<usize>(),
            NAME_WIDTH
        );
        assert_eq!(
            format_name("Claude 桌面版")
                .chars()
                .map(char_display_width)
                .sum::<usize>(),
            NAME_WIDTH
        );
        assert_eq!(
            format_name("abc"),
            format!("abc{}", " ".repeat(NAME_WIDTH - 3))
        );
        assert_eq!(
            format_name("x".repeat(NAME_WIDTH + 1).as_str()),
            format!("{}…", "x".repeat(NAME_WIDTH - 1))
        );
    }
}
