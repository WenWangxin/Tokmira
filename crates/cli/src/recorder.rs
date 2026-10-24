use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use token_audit_core::recorder::{self as core, RecorderParts};

const INPUT_LIMIT: usize = 1024 * 1024;

fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|path| path.join("token-audit"))
}
fn read_input(mut input: impl Read, mut child_stdin: Option<impl Write>) -> Vec<u8> {
    let mut recorded = Vec::new();
    let mut buf = [0_u8; 8192];
    loop {
        let count = match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if recorded.len() <= INPUT_LIMIT {
            let left = INPUT_LIMIT + 1 - recorded.len();
            recorded.extend_from_slice(&buf[..count.min(left)]);
        }
        if let Some(stdin) = child_stdin.as_mut() {
            if stdin.write_all(&buf[..count]).is_err() {
                child_stdin = None;
            }
        }
    }
    recorded
}

#[cfg(windows)]
fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let bash = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|dir| dir.join("bash.exe"))
                .find(|path| path.is_file())
        });
    let mut cmd = if let Some(bash) = bash {
        let mut cmd = Command::new(bash);
        cmd.arg("-c").arg(command);
        cmd
    } else {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    };
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
#[cfg(unix)]
fn shell(command: &str) -> Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(command).process_group(0);
    cmd
}

fn wait_with_timeout(child: &mut Child) -> (ExitCode, bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return (
                    ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
                    false,
                )
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                #[cfg(unix)]
                unsafe {
                    libc::killpg(child.id() as i32, libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                return (ExitCode::FAILURE, true);
            }
        }
    }
}

fn statusline_hook(data_dir: Option<PathBuf>) -> ExitCode {
    let original = if std::env::var_os("TOKEN_AUDIT_STATUSLINE_DEPTH").is_some() {
        None
    } else {
        data_dir
            .as_deref()
            .and_then(core::original_statusline_command)
    };
    if let Some(command) = original {
        let mut child = match shell(&command)
            .env("TOKEN_AUDIT_STATUSLINE_DEPTH", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                let recorded = read_input(io::stdin().lock(), None::<io::Sink>);
                if let Some(dir) = data_dir.as_deref() {
                    core::record_statusline(dir, &recorded);
                }
                return ExitCode::FAILURE;
            }
        };
        let stdin = child.stdin.take();
        let dir = data_dir.clone();
        let input_thread = thread::spawn(move || {
            let recorded = read_input(io::stdin(), stdin);
            if let Some(dir) = dir.as_deref() {
                core::record_statusline(dir, &recorded);
            }
        });
        let output_thread = child.stdout.take().map(|mut stdout| {
            thread::spawn(move || {
                let _ = io::copy(&mut stdout, &mut io::stdout().lock());
            })
        });
        let (exit, timed_out) = wait_with_timeout(&mut child);
        if !timed_out {
            let _ = input_thread.join();
        }
        if !timed_out {
            if let Some(output_thread) = output_thread {
                let _ = output_thread.join();
            }
        }
        exit
    } else {
        let recorded = read_input(io::stdin().lock(), None::<io::Sink>);
        if let Some(dir) = data_dir.as_deref() {
            core::record_statusline(dir, &recorded);
        }
        ExitCode::SUCCESS
    }
}
fn hook_data_dir(args: &[OsString]) -> Option<PathBuf> {
    if args.len() == 2 && args[0] == "--data-dir" {
        Some(PathBuf::from(&args[1]))
    } else if args.is_empty() {
        default_data_dir()
    } else {
        None
    }
}
fn event_hook(args: Vec<OsString>) -> ExitCode {
    if args.len() >= 2 && args[0] == "--tool" && args[1] == "claude" {
        let data_dir = hook_data_dir(&args[2..]);
        let recorded = read_input(io::stdin().lock(), None::<io::Sink>);
        if let Some(dir) = data_dir {
            core::record_hook_event(&dir, &recorded);
        }
    }
    ExitCode::SUCCESS
}

#[derive(Default)]
struct RecorderOptions {
    home: Option<PathBuf>,
    data: Option<PathBuf>,
    exe: Option<PathBuf>,
    parts: RecorderParts,
    dry_run: bool,
    json: bool,
}
fn options(args: impl Iterator<Item = OsString>) -> Result<RecorderOptions, String> {
    let mut options = RecorderOptions::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let flag = arg.to_str().ok_or("参数名不是有效文本")?;
        match flag {
            "--home" | "--data-dir" | "--exe" => {
                let value = args.next().ok_or_else(|| format!("{flag} 缺少路径"))?;
                if value.to_str().is_some_and(|value| value.starts_with("--")) {
                    return Err(format!("{flag} 缺少路径"));
                }
                let slot = match flag {
                    "--home" => &mut options.home,
                    "--data-dir" => &mut options.data,
                    _ => &mut options.exe,
                };
                if slot.replace(PathBuf::from(value)).is_some() {
                    return Err(format!("{flag} 只能指定一次"));
                }
            }
            "--statusline" if !options.parts.statusline => options.parts.statusline = true,
            "--hooks" if !options.parts.hooks => options.parts.hooks = true,
            "--dry-run" if !options.dry_run => options.dry_run = true,
            "--json" if !options.json => options.json = true,
            _ => return Err(format!("未知或重复参数：{flag}")),
        }
    }
    Ok(options)
}
fn resolved(home: Option<PathBuf>, data: Option<PathBuf>) -> Result<(PathBuf, PathBuf), String> {
    let home = home
        .or_else(dirs::home_dir)
        .ok_or("无法确定用户主目录，请指定 --home")?;
    let data = data
        .or_else(default_data_dir)
        .ok_or("无法确定数据目录，请指定 --data-dir")?;
    Ok((home, data))
}
fn print_plan(plan: &core::RecorderPlan) {
    if plan.changes.is_empty() {
        println!("没有需要修改的配置项");
    }
    for change in &plan.changes {
        println!(
            "{}：{} {}",
            change.file.display(),
            change.key_path,
            change.action
        );
        if let Some(before) = &change.before_command {
            println!("  原命令：{before}");
        }
        if let Some(after) = &change.after_command {
            println!("  新命令：{after}");
        }
    }
    for note in &plan.notes {
        println!("{note}");
    }
}
fn run_recorder(action: &str, args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let options = options(args)?;
    let RecorderOptions {
        home,
        data,
        exe,
        mut parts,
        dry_run,
        json,
    } = options;
    let (home, data) = resolved(home, data)?;
    match action {
        "status" if exe.is_none() && !parts.statusline && !parts.hooks && !dry_run => {
            let status = core::status(&home, &data);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).map_err(|_| "生成 JSON 失败")?
                );
            } else {
                println!(
                    "状态栏：{}{}",
                    if status.statusline_installed {
                        "已安装"
                    } else {
                        "未安装"
                    },
                    if status.statusline_modified {
                        "（已被修改）"
                    } else {
                        ""
                    }
                );
                println!(
                    "Hooks：{}{}",
                    if status.hooks_installed {
                        "已安装"
                    } else {
                        "未安装"
                    },
                    if status.hooks_modified {
                        "（已被修改）"
                    } else {
                        ""
                    }
                );
                println!(
                    "状态栏记录：{} 条；Hook 事件：{} 条",
                    status.statusline_records, status.hook_events
                );
            }
        }
        "install" if !json => {
            if !parts.statusline && !parts.hooks {
                parts.statusline = true;
            }
            let exe = exe
                .map(Ok)
                .unwrap_or_else(std::env::current_exe)
                .map_err(|_| "无法确定当前程序路径")?;
            let exe = exe.canonicalize().map_err(|_| "无法解析可执行文件路径")?;
            let plan = core::plan_install(&home, &data, &exe, parts).map_err(|e| e.to_string())?;
            print_plan(&plan);
            if !dry_run {
                let result = core::install(&home, &data, &exe, parts).map_err(|e| e.to_string())?;
                if let Some(path) = result.backup_path {
                    println!("已备份到 {}", path.display());
                }
            }
        }
        "uninstall" if exe.is_none() && !json => {
            if !parts.statusline && !parts.hooks {
                parts.statusline = true;
                parts.hooks = true;
            }
            let plan = core::plan_uninstall(&home, &data, parts).map_err(|e| e.to_string())?;
            print_plan(&plan);
            if !dry_run {
                let result = core::uninstall(&home, &data, parts).map_err(|e| e.to_string())?;
                if let Some(path) = result.backup_path {
                    println!("已备份到 {}", path.display());
                }
            }
        }
        "records" if exe.is_none() && !parts.statusline && !parts.hooks && !dry_run => {
            let (records, stats) = core::read_statusline_records(&data);
            if json {
                let filtered: Vec<_> = records.iter().rev().take(20).map(|r| serde_json::json!({"recorded_at":r.recorded_at,"model":r.model,"five_hour":r.five_hour,"seven_day":r.seven_day})).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"records":filtered,"stats":stats})
                    )
                    .map_err(|_| "生成 JSON 失败")?
                );
            } else {
                for record in records.iter().rev().take(20) {
                    println!(
                        "{}  5 小时：{}  每周：{}",
                        record
                            .recorded_at
                            .with_timezone(&Local)
                            .format("%Y-%m-%d %H:%M"),
                        display_window(record.five_hour.as_ref()),
                        display_window(record.seven_day.as_ref())
                    );
                }
                if stats.malformed_lines > 0 {
                    println!("跳过格式错误记录 {} 行", stats.malformed_lines);
                }
            }
        }
        _ => return Err("此子命令不接受这些参数".to_owned()),
    }
    Ok(())
}
fn display_window(window: Option<&core::QuotaWindow>) -> String {
    window
        .map(|window| {
            format!(
                "{}%，重置 {}",
                window
                    .used_percentage
                    .map_or_else(|| "未知".to_owned(), |v| v.to_string()),
                window.resets_at.map_or_else(
                    || "未知".to_owned(),
                    |v| {
                        Local.timestamp_opt(v, 0).single().map_or_else(
                            || "未知".to_owned(),
                            |time| time.format("%Y-%m-%d %H:%M").to_string(),
                        )
                    }
                )
            )
        })
        .unwrap_or_else(|| "未提供".to_owned())
}
pub fn run(command: &str, args: impl Iterator<Item = OsString>) -> ExitCode {
    let mut args = args;
    if command == "hook" {
        match args.next().as_deref().and_then(|v| v.to_str()) {
            Some("statusline") => statusline_hook(hook_data_dir(&args.collect::<Vec<_>>())),
            Some("event") => event_hook(args.collect()),
            _ => ExitCode::SUCCESS,
        }
    } else {
        let Some(action) = args.next() else {
            eprintln!("缺少 recorder 子命令");
            return ExitCode::FAILURE;
        };
        let Some(action) = action.to_str() else {
            eprintln!("子命令不是有效文本");
            return ExitCode::FAILURE;
        };
        match run_recorder(action, args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    }
}
