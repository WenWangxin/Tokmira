use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use token_audit_core::claude::{scan_claude, ClaudeDebugSummary};

fn usage() {
    println!(
        "用法：token-audit debug claude [--home <目录>] [--json]\n       token-audit --version"
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

fn print_summary(summary: &ClaudeDebugSummary) {
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

fn run_debug(args: impl Iterator<Item = OsString>) -> Result<(), &'static str> {
    let (home, json) = parse_debug_args(args)?;
    let scan = scan_claude(&home).map_err(|_| "扫描 Claude 日志失败")?;
    let summary = scan.debug_summary();
    if json {
        let output = serde_json::to_string_pretty(&summary).map_err(|_| "生成 JSON 失败")?;
        println!("{output}");
    } else {
        print_summary(&summary);
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
        Some("debug") if args.next().as_deref().and_then(|arg| arg.to_str()) == Some("claude") => {
            match run_debug(args) {
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
