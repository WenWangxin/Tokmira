#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::State;
use token_audit_core::attribution::{attribute, Attribution, WindowKind};
use token_audit_core::claude::{scan_claude, ClaudeScan};
use token_audit_core::claude_quota::{
    query_claude_quota, system_proxy, ClaudeQuotaError, QueryOptions, SystemSecretStore,
    UreqTransport,
};
use token_audit_core::codex::{scan_codex, CodexScan};
use token_audit_core::codex_app_server::{
    codex_candidates, read_codex_quota, resolve_launch, AppServerOptions, CodexQuotaError,
};
use token_audit_core::dashboard::{build_overview, Overview, OverviewInput};
use token_audit_core::model::Tool;
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{self, CodexDaily};
use token_audit_core::recorder::{self, RecorderParts, RecorderPlan, RecorderStatus};
use token_audit_core::settings::{self, AppSettings};
use token_audit_core::timeline::build_timeline;
use token_audit_core::usage::{list_usage_windows, usage_detail, UsageDetail, UsageWindowRef};

#[derive(Clone)]
struct Scans {
    claude: ClaudeScan,
    codex: CodexScan,
}
#[derive(Default)]
struct RefreshClock {
    last_claude: Option<DateTime<Utc>>,
    last_codex: Option<DateTime<Utc>>,
    claude_retry_at: Option<DateTime<Utc>>,
}
struct AppState {
    home: PathBuf,
    data_dir: PathBuf,
    scans: Arc<Mutex<Option<Scans>>>,
    refresh: Arc<Mutex<RefreshClock>>,
}
fn scan(home: &std::path::Path) -> Result<Scans, String> {
    Ok(Scans {
        claude: scan_claude(home).map_err(|_| "扫描 Claude 日志失败".to_owned())?,
        codex: scan_codex(home).map_err(|_| "扫描 Codex 日志失败".to_owned())?,
    })
}
fn lock_error() -> String {
    "应用状态暂不可用".to_owned()
}
fn cli_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|_| "无法定位桌面应用程序".to_owned())?;
    let name = if cfg!(windows) {
        "token-audit.exe"
    } else {
        "token-audit"
    };
    let path = current.parent().ok_or("无法定位桌面应用目录")?.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("未找到随应用分发的命令行程序：{}", path.display()))
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    settings::read_settings(&state.data_dir).map_err(|_| "读取设置失败".to_owned())
}
#[tauri::command]
fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    settings::write_settings(&state.data_dir, &settings).map_err(|_| "保存设置失败".to_owned())?;
    Ok(settings)
}
#[tauri::command]
async fn rescan(state: State<'_, AppState>) -> Result<ScanSummary, String> {
    let home = state.home.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        let fresh = scan(&home)?;
        let summary = ScanSummary {
            events: fresh.claude.events.len() + fresh.codex.events.len(),
            prompts: fresh.claude.prompts.len() + fresh.codex.prompts.len(),
            malformed_lines: fresh.claude.stats.malformed_lines + fresh.codex.stats.malformed_lines,
        };
        *cached.lock().map_err(|_| lock_error())? = Some(fresh);
        Ok(summary)
    })
    .await
    .map_err(|_| "扫描任务失败".to_owned())?
}
#[derive(Serialize)]
struct ScanSummary {
    events: usize,
    prompts: usize,
    malformed_lines: u64,
}

#[derive(Serialize)]
struct RefreshResult {
    claude: String,
    codex: String,
}
fn proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .or_else(system_proxy)
}
#[tauri::command]
async fn refresh_quotas(state: State<'_, AppState>) -> Result<RefreshResult, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let refresh = Arc::clone(&state.refresh);
    tauri::async_runtime::spawn_blocking(move || {
        let settings = settings::read_settings(&data_dir).map_err(|_| "读取设置失败".to_owned())?;
        let mut clock = refresh.lock().map_err(|_| lock_error())?;
        let now = Utc::now();
        let mut result = RefreshResult {
            claude: "未开启".to_owned(),
            codex: "未开启".to_owned(),
        };
        if settings.claude_official_query {
            if clock.claude_retry_at.is_some_and(|at| now < at) {
                result.claude = "限流退避中".to_owned();
            } else if clock
                .last_claude
                .is_some_and(|at| now - at < chrono::Duration::seconds(60))
            {
                result.claude = "60 秒内已查询".to_owned();
            } else {
                clock.last_claude = Some(now);
                let options = QueryOptions {
                    preference: settings.claude_credential,
                    proxy: proxy(),
                    timeout: Duration::from_secs(15),
                    keychain_timeout: Duration::from_secs(60),
                };
                match query_claude_quota(
                    &SystemSecretStore::default(),
                    &home,
                    &UreqTransport,
                    &options,
                ) {
                    Ok(quota) => {
                        quota_store::append_claude(&data_dir, &(&quota).into())
                            .map_err(|_| "保存 Claude 额度快照失败".to_owned())?;
                        result.claude = "已更新".to_owned();
                    }
                    Err(ClaudeQuotaError::RateLimited { retry_after }) => {
                        clock.claude_retry_at = Some(
                            now + chrono::Duration::from_std(
                                retry_after.unwrap_or(Duration::from_secs(300)),
                            )
                            .unwrap_or(chrono::Duration::minutes(5)),
                        );
                        result.claude = "Anthropic 查询已限流".to_owned();
                    }
                    Err(error) => result.claude = error.to_string(),
                }
            }
        }
        if settings.codex_app_server {
            if clock
                .last_codex
                .is_some_and(|at| now - at < chrono::Duration::seconds(60))
            {
                result.codex = "60 秒内已查询".to_owned();
            } else {
                clock.last_codex = Some(now);
                result.codex = match codex_candidates(&home, std::env::var_os("PATH").as_deref())
                    .into_iter()
                    .next()
                {
                    Some(path) => {
                        match read_codex_quota(&resolve_launch(&path), &AppServerOptions::default())
                        {
                            Ok(quota) => {
                                quota_store::append_codex(&data_dir, &(&quota).into())
                                    .map_err(|_| "保存 Codex 额度快照失败".to_owned())?;
                                if let Some(usage) = quota.usage {
                                    let days: Vec<_> = usage
                                        .daily
                                        .into_iter()
                                        .map(|d| CodexDaily {
                                            date: d.date,
                                            tokens: d.tokens,
                                        })
                                        .collect();
                                    quota_store::write_codex_daily(&data_dir, &days)
                                        .map_err(|_| "保存 Codex 每日用量失败".to_owned())?;
                                }
                                "已更新".to_owned()
                            }
                            Err(CodexQuotaError::Rpc { code, .. }) => {
                                format!("Codex app-server 错误代码 {code}")
                            }
                            Err(error) => error.to_string(),
                        }
                    }
                    None => "未找到 Codex 命令行程序".to_owned(),
                };
            }
        }
        Ok(result)
    })
    .await
    .map_err(|_| "额度查询任务失败".to_owned())?
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        let scans = {
            let mut guard = cached.lock().map_err(|_| lock_error())?;
            if guard.is_none() {
                *guard = Some(scan(&home)?);
            }
            guard.as_ref().ok_or_else(lock_error)?.clone()
        };
        let (claude_snapshots, _) = quota_store::read_claude(&data_dir);
        let (codex_snapshots, _) = quota_store::read_codex(&data_dir);
        let (statusline, _) = recorder::read_statusline_records(&data_dir);
        let settings = settings::read_settings(&data_dir).map_err(|_| "读取设置失败".to_owned())?;
        let points = build_timeline(
            &claude_snapshots,
            &codex_snapshots,
            &statusline,
            &scans.codex.rate_limits,
            settings.calibration.as_ref(),
        );
        let pricing = Pricing::bundled().map_err(str::to_owned)?;
        let mut events = scans.claude.events;
        events.extend(scans.codex.events);
        let mut prompts = scans.claude.prompts;
        prompts.extend(scans.codex.prompts);
        let mut titles: BTreeMap<_, _> = scans.claude.titles;
        titles.extend(scans.codex.titles);
        let attribution = attribute(
            &events,
            &prompts,
            &titles,
            &points,
            &claude_snapshots,
            &pricing,
        );
        Ok(build_overview(
            &OverviewInput {
                events: &events,
                titles: &titles,
                points: &points,
                attribution: &attribution,
                claude_snapshots: &claude_snapshots,
                codex_snapshots: &codex_snapshots,
                codex_log: &scans.codex.rate_limits,
                pricing: &pricing,
                recorder: recorder::status(&home, &data_dir),
            },
            Utc::now(),
            chrono::Local,
        ))
    })
    .await
    .map_err(|_| "生成仪表盘失败".to_owned())?
}

#[tauri::command]
async fn get_attribution(state: State<'_, AppState>) -> Result<Attribution, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        let scans = {
            let mut guard = cached.lock().map_err(|_| lock_error())?;
            if guard.is_none() {
                *guard = Some(scan(&home)?);
            }
            guard.as_ref().ok_or_else(lock_error)?.clone()
        };
        let (claude_snapshots, _) = quota_store::read_claude(&data_dir);
        let (codex_snapshots, _) = quota_store::read_codex(&data_dir);
        let (statusline, _) = recorder::read_statusline_records(&data_dir);
        let settings = settings::read_settings(&data_dir).map_err(|_| "读取设置失败".to_owned())?;
        let points = build_timeline(
            &claude_snapshots,
            &codex_snapshots,
            &statusline,
            &scans.codex.rate_limits,
            settings.calibration.as_ref(),
        );
        let pricing = Pricing::bundled().map_err(str::to_owned)?;
        let mut events = scans.claude.events;
        events.extend(scans.codex.events);
        let mut prompts = scans.claude.prompts;
        prompts.extend(scans.codex.prompts);
        let mut titles: BTreeMap<_, _> = scans.claude.titles;
        titles.extend(scans.codex.titles);
        Ok(attribute(
            &events,
            &prompts,
            &titles,
            &points,
            &claude_snapshots,
            &pricing,
        ))
    })
    .await
    .map_err(|_| "生成额度归因失败".to_owned())?
}

fn with_usage<R>(
    home: &std::path::Path,
    data_dir: &std::path::Path,
    cached: &Mutex<Option<Scans>>,
    build: impl FnOnce(&OverviewInput<'_>, DateTime<Utc>) -> Result<R, String>,
) -> Result<R, String> {
    let scans = {
        let mut guard = cached.lock().map_err(|_| lock_error())?;
        if guard.is_none() {
            *guard = Some(scan(home)?);
        }
        guard.as_ref().ok_or_else(lock_error)?.clone()
    };
    let (claude_snapshots, _) = quota_store::read_claude(data_dir);
    let (codex_snapshots, _) = quota_store::read_codex(data_dir);
    let (statusline, _) = recorder::read_statusline_records(data_dir);
    let settings = settings::read_settings(data_dir).map_err(|_| "读取设置失败".to_owned())?;
    let points = build_timeline(
        &claude_snapshots,
        &codex_snapshots,
        &statusline,
        &scans.codex.rate_limits,
        settings.calibration.as_ref(),
    );
    let pricing = Pricing::bundled().map_err(str::to_owned)?;
    let mut events = scans.claude.events;
    events.extend(scans.codex.events);
    let mut prompts = scans.claude.prompts;
    prompts.extend(scans.codex.prompts);
    let mut titles: BTreeMap<_, _> = scans.claude.titles;
    titles.extend(scans.codex.titles);
    let attribution = attribute(
        &events,
        &prompts,
        &titles,
        &points,
        &claude_snapshots,
        &pricing,
    );
    let input = OverviewInput {
        events: &events,
        titles: &titles,
        points: &points,
        attribution: &attribution,
        claude_snapshots: &claude_snapshots,
        codex_snapshots: &codex_snapshots,
        codex_log: &scans.codex.rate_limits,
        pricing: &pricing,
        recorder: recorder::status(home, data_dir),
    };
    build(&input, Utc::now())
}

#[tauri::command]
async fn get_usage_windows(
    state: State<'_, AppState>,
    tool: Tool,
) -> Result<Vec<UsageWindowRef>, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |input, now| {
            Ok(list_usage_windows(input, tool, now, 6))
        })
    })
    .await
    .map_err(|_| "生成用量窗口失败".to_owned())?
}

#[tauri::command]
async fn get_usage_detail(
    state: State<'_, AppState>,
    tool: Tool,
    kind: WindowKind,
    window_id: DateTime<Utc>,
) -> Result<UsageDetail, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |input, now| {
            let window = list_usage_windows(input, tool, now, usize::MAX)
                .into_iter()
                .find(|w| w.kind == kind && w.window_id == window_id)
                .ok_or_else(|| "指定的用量窗口不存在".to_owned())?;
            Ok(usage_detail(input, &window, now))
        })
    })
    .await
    .map_err(|_| "生成用量明细失败".to_owned())?
}
#[tauri::command]
fn recorder_status(state: State<'_, AppState>) -> RecorderStatus {
    recorder::status(&state.home, &state.data_dir)
}
#[tauri::command]
fn recorder_plan(state: State<'_, AppState>, parts: RecorderParts) -> Result<RecorderPlan, String> {
    let exe = cli_executable()?;
    recorder::plan_install(&state.home, &state.data_dir, &exe, parts)
        .map_err(|error| error.to_string())
}
#[tauri::command]
fn recorder_install(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    let exe = cli_executable()?;
    recorder::install(&state.home, &state.data_dir, &exe, parts).map_err(|error| error.to_string())
}
#[tauri::command]
fn recorder_uninstall(state: State<'_, AppState>) -> Result<RecorderPlan, String> {
    recorder::uninstall(&state.home, &state.data_dir).map_err(|error| error.to_string())
}

fn main() -> tauri::Result<()> {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            let home =
                dirs::home_dir().ok_or_else(|| std::io::Error::other("无法确定用户主目录"))?;
            let data_dir = dirs::data_dir()
                .ok_or_else(|| std::io::Error::other("无法确定应用数据目录"))?
                .join("token-audit");
            app.manage(AppState {
                home,
                data_dir,
                scans: Arc::new(Mutex::new(None)),
                refresh: Arc::new(Mutex::new(RefreshClock::default())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            rescan,
            refresh_quotas,
            get_overview,
            get_attribution,
            get_usage_windows,
            get_usage_detail,
            recorder_status,
            recorder_plan,
            recorder_install,
            recorder_uninstall
        ])
        .run(tauri::generate_context!())
}
