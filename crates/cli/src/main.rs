use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use token_audit_core::claude::{scan_claude, ClaudeDebugSummary};
use token_audit_core::codex::{scan_codex, CodexDebugSummary, RateLimitWindow};

fn usage() {
    println!(
        "用法：token-audit debug <claude|codex> [--home <目录>] [--json]\n       token-audit --version"
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
        _ => {
            eprintln!("未知参数。");
            usage();
            ExitCode::FAILURE
        }
    }
}
