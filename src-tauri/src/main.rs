#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod login_item;
mod notifications;
mod realtime;
mod tray;
mod tray_menu;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{Emitter, State};
use token_audit_core::advice::{
    build_advice, AdviceAlert, AdviceConfig, AdviceInput, AdviceReport, AlertTracker,
};
use token_audit_core::attribution::{attribute, Attribution, WindowKind};
use token_audit_core::claude::{ClaudeScan, ClaudeScanner};
use token_audit_core::claude_quota::{
    query_claude_quota, system_proxy, ClaudeQuotaError, MemorySecretStore, QueryOptions,
    SystemSecretStore, UreqTransport,
};
use token_audit_core::codex::{CodexScan, CodexScanner};
use token_audit_core::codex_app_server::{
    codex_candidates, read_codex_quota, resolve_launch, AppServerOptions, CodexQuotaError,
};
use token_audit_core::dashboard::{build_overview, Overview, OverviewInput};
use token_audit_core::detect::{self, DetectedTool};
use token_audit_core::model::{PromptEvent, Tool};
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{self, CodexDaily};
use token_audit_core::recorder::{self, RecorderParts, RecorderPlan, RecorderStatus};
use token_audit_core::sessions::{
    list_sessions, session_detail, SessionDetail, SessionInput, SessionRow,
};
use token_audit_core::settings::{self, AppSettings};
use token_audit_core::share::{self, share_summary, ShareSummary};
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
    /// The keychain read was denied or timed out. Automatic queries stop asking
    /// until the user refreshes by hand, switches the query on again, or restarts,
    /// so a dismissed password dialog does not come back every minute.
    claude_awaits_user: bool,
    claude_needs_authorization: bool,
}

/// Result of a background Claude query that could not read the credential without a
/// prompt. The UI shows it as a button rather than an error.
const NEEDS_AUTHORIZATION: &str = "需要授权";

/// Whether a failed Claude query got as far as a request to Anthropic.
fn reached_anthropic(error: &ClaudeQuotaError) -> bool {
    !matches!(
        error,
        ClaudeQuotaError::NoCredential
            | ClaudeQuotaError::CredentialDenied { .. }
            | ClaudeQuotaError::KeychainTimeout { .. }
            | ClaudeQuotaError::TokenExpired { .. }
            | ClaudeQuotaError::Unsupported
    )
}

/// Why a Claude query is skipped now, if it is. Pure, so the polling rules are testable.
fn claude_skip_reason(
    clock: &RefreshClock,
    now: DateTime<Utc>,
    automatic: bool,
) -> Option<&'static str> {
    if automatic && clock.claude_awaits_user {
        return Some(NEEDS_AUTHORIZATION);
    }
    if clock.claude_retry_at.is_some_and(|at| now < at) {
        return Some("限流退避中");
    }
    if clock
        .last_claude
        .is_some_and(|at| now - at < chrono::Duration::seconds(60))
    {
        return Some("60 秒内已查询");
    }
    None
}
#[derive(Clone)]
struct AppState {
    home: PathBuf,
    data_dir: PathBuf,
    scans: Arc<Mutex<ScanCache>>,
    refresh: Arc<Mutex<RefreshClock>>,
    /// Keychain secrets read for the Claude quota query, kept in memory for this
    /// run only so periodic queries do not prompt again. Never written to disk.
    secrets: MemorySecretStore<SystemSecretStore>,
}
/// The latest scan plus the incremental scanners that produced it; a rescan only
/// reparses log files that changed since the previous one.
#[derive(Default)]
struct ScanCache {
    claude: ClaudeScanner,
    codex: CodexScanner,
    latest: Option<Scans>,
}
impl ScanCache {
    /// Rescans changed files and reports which tools' scans changed.
    fn refresh(&mut self, home: &std::path::Path) -> Result<(bool, bool), String> {
        let claude = self
            .claude
            .scan(home)
            .map_err(|_| "扫描 Claude 日志失败".to_owned())?;
        let codex = self
            .codex
            .scan(home)
            .map_err(|_| "扫描 Codex 日志失败".to_owned())?;
        let first = self.latest.is_none();
        self.latest = Some(Scans { claude, codex });
        Ok((
            first || self.claude.changed(),
            first || self.codex.changed(),
        ))
    }
    fn current(&mut self, home: &std::path::Path) -> Result<&Scans, String> {
        if self.latest.is_none() {
            self.refresh(home)?;
        }
        self.latest.as_ref().ok_or_else(lock_error)
    }
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<AppSettings, String> {
    let current = settings::read_settings(&state.data_dir).ok();
    let was_on = current
        .as_ref()
        .is_some_and(|current| current.claude_official_query);
    // Set by the backend when the window first hides; a page saving an older copy must
    // not bring the notice back.
    settings.tray_hint_shown |= current.is_some_and(|current| current.tray_hint_shown);
    settings::write_settings(&state.data_dir, &settings).map_err(|_| "保存设置失败".to_owned())?;
    // Switching the query on again is the user asking to retry the keychain.
    if settings.claude_official_query && !was_on {
        if let Ok(mut clock) = state.refresh.lock() {
            clock.claude_awaits_user = false;
            clock.claude_needs_authorization = false;
        }
    }
    tray::signal(&app);
    Ok(settings)
}
#[tauri::command]
async fn rescan(state: State<'_, AppState>) -> Result<ScanSummary, String> {
    let home = state.home.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = cached.lock().map_err(|_| lock_error())?;
        guard.refresh(&home)?;
        let fresh = guard.current(&home)?;
        Ok(ScanSummary {
            events: fresh.claude.events.len() + fresh.codex.events.len(),
            prompts: fresh.claude.prompts.len() + fresh.codex.prompts.len(),
            malformed_lines: fresh.claude.stats.malformed_lines + fresh.codex.stats.malformed_lines,
        })
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
fn refresh_quotas_sync(state: &AppState, automatic: bool) -> Result<RefreshResult, String> {
    let settings =
        settings::read_settings(&state.data_dir).map_err(|_| "读取设置失败".to_owned())?;
    let mut clock = state.refresh.lock().map_err(|_| lock_error())?;
    let now = Utc::now();
    let mut result = RefreshResult {
        claude: "未开启".to_owned(),
        codex: "未开启".to_owned(),
    };
    if settings.claude_official_query {
        if let Some(reason) = claude_skip_reason(&clock, now, automatic) {
            result.claude = reason.to_owned();
        } else {
            let previous_query = clock.last_claude;
            clock.last_claude = Some(now);
            clock.claude_awaits_user = false;
            let options = QueryOptions {
                preference: settings.claude_credential,
                proxy: proxy(),
                timeout: Duration::from_secs(15),
                keychain_timeout: Duration::from_secs(60),
            };
            // Background polls must never open a keychain dialog; they only reuse
            // what this run already holds or what the user set to "always allow".
            let store = if automatic {
                state.secrets.silent()
            } else {
                state.secrets.clone()
            };
            match query_claude_quota(&store, &state.home, &UreqTransport, &options) {
                Ok(quota) => {
                    quota_store::append_claude(&state.data_dir, &(&quota).into())
                        .map_err(|_| "保存 Claude 额度快照失败".to_owned())?;
                    clock.claude_needs_authorization = false;
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
                Err(error) => {
                    if error.stale_credential() {
                        state.secrets.forget();
                    }
                    // The minimum interval protects Anthropic; a query that stopped
                    // at the credential never reached it, so it does not count and
                    // the user's "authorize" click right after a silent poll goes ahead.
                    if !reached_anthropic(&error) {
                        clock.last_claude = previous_query;
                    }
                    if matches!(
                        error,
                        ClaudeQuotaError::CredentialDenied { .. }
                            | ClaudeQuotaError::KeychainTimeout { .. }
                    ) {
                        clock.claude_awaits_user = true;
                    }
                    // A silent read found nothing it may use without asking: the
                    // window offers the user a button that asks once, by hand.
                    result.claude = if automatic
                        && matches!(
                            error,
                            ClaudeQuotaError::NoCredential
                                | ClaudeQuotaError::CredentialDenied { .. }
                        ) {
                        NEEDS_AUTHORIZATION.to_owned()
                    } else {
                        error.to_string()
                    };
                    if result.claude == NEEDS_AUTHORIZATION {
                        clock.claude_needs_authorization = true;
                    }
                }
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
            result.codex = match codex_candidates(&state.home, std::env::var_os("PATH").as_deref())
                .into_iter()
                .next()
            {
                Some(path) => {
                    match read_codex_quota(&resolve_launch(&path), &AppServerOptions::default()) {
                        Ok(quota) => {
                            quota_store::append_codex(&state.data_dir, &(&quota).into())
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
                                quota_store::write_codex_daily(&state.data_dir, &days)
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
}

#[tauri::command]
async fn refresh_quotas(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    automatic: Option<bool>,
) -> Result<RefreshResult, String> {
    let state = state.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        refresh_quotas_sync(&state, automatic.unwrap_or(false))
    })
    .await
    .map_err(|_| "额度查询任务失败".to_owned())?;
    tray::signal(&app);
    result
}

fn overview_sync(state: &AppState) -> Result<Overview, String> {
    with_usage(&state.home, &state.data_dir, &state.scans, |context| {
        Ok(build_overview(context.input, context.now, chrono::Local))
    })
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || overview_sync(&state))
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
            cached
                .lock()
                .map_err(|_| lock_error())?
                .current(&home)?
                .clone()
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

/// Everything a page command needs, computed once from the cached scan.
struct UsageContext<'a> {
    input: &'a OverviewInput<'a>,
    prompts: &'a [PromptEvent],
    settings: &'a AppSettings,
    now: DateTime<Utc>,
}

#[tauri::command]
async fn get_share_summary(state: State<'_, AppState>) -> Result<ShareSummary, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            Ok(share_summary(context.input, context.now, chrono::Local))
        })
    })
    .await
    .map_err(|_| "生成分享摘要失败".to_owned())?
}

#[tauri::command]
fn save_export(file_name: String, bytes: Vec<u8>) -> Result<String, String> {
    let directory = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "无法确定下载目录".to_owned())?;
    let path = share::save_export_file(&directory, &file_name, &bytes).map_err(str::to_owned)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn detect_tools(state: State<'_, AppState>) -> Result<Vec<DetectedTool>, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            Ok(detect::detect_tools(
                context.input.events,
                context.input.codex_log,
                context.input.claude_snapshots,
            ))
        })
    })
    .await
    .map_err(|_| "检测本机工具失败".to_owned())?
}

fn with_usage<R>(
    home: &std::path::Path,
    data_dir: &std::path::Path,
    cached: &Mutex<ScanCache>,
    build: impl FnOnce(&UsageContext<'_>) -> Result<R, String>,
) -> Result<R, String> {
    let scans = {
        cached
            .lock()
            .map_err(|_| lock_error())?
            .current(home)?
            .clone()
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
    build(&UsageContext {
        input: &input,
        prompts: &prompts,
        settings: &settings,
        now: Utc::now(),
    })
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
        with_usage(&home, &data_dir, &cached, |context| {
            let (input, now) = (context.input, context.now);
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
        with_usage(&home, &data_dir, &cached, |context| {
            let (input, now) = (context.input, context.now);
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
#[derive(Serialize)]
struct DirectoryStatus {
    path: PathBuf,
    exists: bool,
}
#[derive(Serialize)]
struct AppEnvironment {
    claude_logs: DirectoryStatus,
    codex_logs: DirectoryStatus,
    data_dir: PathBuf,
    pricing_version: String,
    pricing_note: String,
    advice_version: String,
    backup_dir: PathBuf,
}
#[tauri::command]
fn get_environment(state: State<'_, AppState>) -> Result<AppEnvironment, String> {
    let pricing = Pricing::bundled().map_err(str::to_owned)?;
    let advice = AdviceConfig::bundled().map_err(str::to_owned)?;
    let claude_logs = state.home.join(".claude").join("projects");
    let codex_logs = state.home.join(".codex").join("sessions");
    Ok(AppEnvironment {
        claude_logs: DirectoryStatus {
            exists: claude_logs.exists(),
            path: claude_logs,
        },
        codex_logs: DirectoryStatus {
            exists: codex_logs.exists(),
            path: codex_logs,
        },
        data_dir: state.data_dir.clone(),
        pricing_version: pricing.version,
        pricing_note: pricing.note,
        advice_version: advice.version,
        backup_dir: state.data_dir.join("recorder").join("backups"),
    })
}
#[tauri::command]
fn clear_recorder_records(state: State<'_, AppState>) -> Result<(), String> {
    recorder::clear_records(&state.data_dir).map_err(|error| error.to_string())
}
#[tauri::command]
fn recorder_plan_uninstall(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    recorder::plan_uninstall(&state.home, &state.data_dir, parts).map_err(|error| error.to_string())
}
#[tauri::command]
fn recorder_uninstall(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    recorder::uninstall(&state.home, &state.data_dir, parts).map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_advice(state: State<'_, AppState>, period_days: u32) -> Result<AdviceReport, String> {
    if period_days != 7 && period_days != 30 {
        return Err("统计天数只能是 7 或 30".to_owned());
    }
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            advice_report(context, period_days)
        })
    })
    .await
    .map_err(|_| "生成省额度建议失败".to_owned())?
}

fn session_input<'a>(context: &'a UsageContext<'a>) -> SessionInput<'a> {
    SessionInput {
        events: &context.input.attribution.events,
        titles: context.input.titles,
        pricing: context.input.pricing,
    }
}

fn advice_report(context: &UsageContext<'_>, period_days: u32) -> Result<AdviceReport, String> {
    let config = AdviceConfig::bundled().map_err(str::to_owned)?;
    Ok(build_advice(
        &AdviceInput {
            events: &context.input.attribution.events,
            prompts: context.prompts,
            pricing: context.input.pricing,
            config: &config,
            dismissed: &context.settings.dismissed_advice,
        },
        period_days,
        context.now,
    ))
}

#[tauri::command]
async fn get_sessions(state: State<'_, AppState>, days: u32) -> Result<Vec<SessionRow>, String> {
    if days != 7 && days != 30 {
        return Err("统计天数只能是 7 或 30".to_owned());
    }
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            Ok(list_sessions(&session_input(context), days, context.now))
        })
    })
    .await
    .map_err(|_| "生成会话列表失败".to_owned())?
}

#[tauri::command]
async fn get_session_detail(
    state: State<'_, AppState>,
    tool: Tool,
    session_id: String,
) -> Result<SessionDetail, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            let tags = advice_report(context, 30)?.event_tags;
            session_detail(
                &session_input(context),
                context.prompts,
                &tags,
                tool,
                &session_id,
            )
            .ok_or_else(|| "未找到该会话".to_owned())
        })
    })
    .await
    .map_err(|_| "生成会话详情失败".to_owned())?
}

fn set_dismissed(state: &AppState, key: &str, dismissed: bool) -> Result<AppSettings, String> {
    let mut settings =
        settings::read_settings(&state.data_dir).map_err(|_| "读取设置失败".to_owned())?;
    settings.dismissed_advice.retain(|item| item != key);
    if dismissed {
        settings.dismissed_advice.push(key.to_owned());
    }
    settings::write_settings(&state.data_dir, &settings).map_err(|_| "保存设置失败".to_owned())?;
    Ok(settings)
}

#[tauri::command]
fn dismiss_advice(state: State<'_, AppState>, key: String) -> Result<AppSettings, String> {
    set_dismissed(&state, &key, true)
}

#[tauri::command]
fn restore_advice(state: State<'_, AppState>, key: String) -> Result<AppSettings, String> {
    set_dismissed(&state, &key, false)
}

/// Rescans for the watcher and forwards its changes to the window.
struct LiveUpdates {
    app: tauri::AppHandle,
    home: PathBuf,
    scans: Arc<Mutex<ScanCache>>,
    alerts: mpsc::Sender<()>,
}

impl realtime::Sink for LiveUpdates {
    fn rescan(&self) -> (bool, bool) {
        self.scans
            .lock()
            .ok()
            .and_then(|mut cache| cache.refresh(&self.home).ok())
            .unwrap_or((false, false))
    }

    fn notify(&self, change: realtime::UsageChanged) {
        let _ = self.app.emit("usage-changed", change);
        let _ = self.alerts.send(());
        tray::signal(&self.app);
    }
}

/// Checks the advice rules for background reminders on its own thread, so the watcher
/// never waits for it. Signals that pile up during a check collapse into one. After the
/// last change it checks once more when a quiet session lets a light task be judged.
fn spawn_alerts(
    app: tauri::AppHandle,
    home: PathBuf,
    data_dir: PathBuf,
    scans: Arc<Mutex<ScanCache>>,
) -> mpsc::Sender<()> {
    let (sender, receiver) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let Ok(config) = AdviceConfig::bundled() else {
            return;
        };
        let quiet = Duration::from_secs(
            u64::try_from(config.alerts.light_task_settle_minutes).unwrap_or(0) * 60 + 5,
        );
        let mut tracker = AlertTracker::default();
        let mut run = |seed: bool| {
            let alert = with_usage(&home, &data_dir, &scans, |context| {
                let input = AdviceInput {
                    events: &context.input.attribution.events,
                    prompts: context.prompts,
                    pricing: context.input.pricing,
                    config: &config,
                    dismissed: &context.settings.dismissed_advice,
                };
                if seed {
                    tracker.seed(&input, context.now);
                    return Ok(None);
                }
                // Checked even when switched off, so turning it on later does not report old problems.
                let alert = tracker.check(&input, context.now);
                Ok(alert.filter(|_| context.settings.advice_notifications))
            });
            if let Ok(Some(alert)) = alert {
                deliver_alert(&app, &alert);
            }
        };
        // Problems already in the logs at launch are history, not news.
        run(true);
        let mut recheck = false;
        loop {
            let signal = if recheck {
                receiver.recv_timeout(quiet)
            } else {
                receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            match signal {
                Ok(()) => {
                    while receiver.try_recv().is_ok() {}
                    recheck = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => recheck = false,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            run(false);
        }
    });
    sender
}

/// An in-app message while the window has focus (macOS hides notifications from the
/// frontmost app), otherwise a system notification.
fn deliver_alert(app: &tauri::AppHandle, alert: &AdviceAlert) {
    use tauri::Manager;
    let focused = app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
    });
    if focused {
        let _ = app.emit("advice-alert", alert);
    } else {
        notifications::post(app, &alert.title, &alert.body);
    }
}

#[tauri::command]
async fn notification_status() -> notifications::NotificationStatus {
    tauri::async_runtime::spawn_blocking(notifications::status)
        .await
        .unwrap_or(notifications::NotificationStatus::Unknown)
}

#[tauri::command]
async fn request_notifications() -> notifications::NotificationStatus {
    tauri::async_runtime::spawn_blocking(notifications::request)
        .await
        .unwrap_or(notifications::NotificationStatus::Unknown)
}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> login_item::AutostartState {
    login_item::get(&app)
}

#[tauri::command]
fn set_autostart(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<login_item::AutostartState, String> {
    let result = login_item::set(&app, enabled)?;
    tray::signal(&app);
    let _ = app.emit("settings-changed", ());
    Ok(result)
}

#[tauri::command]
fn open_login_items_settings() -> Result<(), String> {
    login_item::open_settings()
}

fn main() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("token-audit")
                .build(),
        )
        .setup(|app| {
            use tauri::Manager;
            let home =
                dirs::home_dir().ok_or_else(|| std::io::Error::other("无法确定用户主目录"))?;
            let data_dir = dirs::data_dir()
                .ok_or_else(|| std::io::Error::other("无法确定应用数据目录"))?
                .join("token-audit");
            let scans = Arc::new(Mutex::new(ScanCache::default()));
            let alerts = spawn_alerts(
                app.handle().clone(),
                home.clone(),
                data_dir.clone(),
                Arc::clone(&scans),
            );
            realtime::spawn(
                home.clone(),
                data_dir.clone(),
                LiveUpdates {
                    app: app.handle().clone(),
                    home: home.clone(),
                    scans: Arc::clone(&scans),
                    alerts,
                },
            );
            app.manage(AppState {
                home: home.clone(),
                data_dir: data_dir.clone(),
                scans,
                refresh: Arc::new(Mutex::new(RefreshClock::default())),
                secrets: MemorySecretStore::new(SystemSecretStore::default()),
            });
            login_item::migrate(app.handle(), &home);
            tray::install(app)?;
            notifications::install(app.handle());
            let launch_settings = settings::read_settings(&data_dir).unwrap_or_default();
            if launch_settings.onboarding_completed && launch_settings.silent_start {
                #[cfg(target_os = "macos")]
                app.handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
            } else if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                api.prevent_close();
                tray::hide_main(window, &window.state::<AppState>().data_dir);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            detect_tools,
            rescan,
            refresh_quotas,
            get_overview,
            get_share_summary,
            save_export,
            get_attribution,
            get_usage_windows,
            get_usage_detail,
            get_advice,
            get_sessions,
            get_session_detail,
            dismiss_advice,
            restore_advice,
            get_environment,
            clear_recorder_records,
            recorder_status,
            recorder_plan,
            recorder_plan_uninstall,
            recorder_install,
            recorder_uninstall,
            notification_status,
            request_notifications,
            get_autostart,
            set_autostart,
            open_login_items_settings
        ])
        .build(tauri::generate_context!())?
        .run(|_app, _event| {
            // Clicking the Dock icon brings the hidden window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                tray::show_main(_app);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_requests_that_reached_anthropic_count_toward_the_interval() {
        use token_audit_core::claude_quota::CredentialSource;
        assert!(!reached_anthropic(&ClaudeQuotaError::NoCredential));
        assert!(!reached_anthropic(&ClaudeQuotaError::CredentialDenied {
            source: CredentialSource::ClaudeDesktop
        }));
        assert!(!reached_anthropic(&ClaudeQuotaError::TokenExpired {
            source: CredentialSource::ClaudeCode
        }));
        assert!(reached_anthropic(&ClaudeQuotaError::Network));
        assert!(reached_anthropic(&ClaudeQuotaError::Http { status: 500 }));
        assert!(reached_anthropic(&ClaudeQuotaError::Unauthorized {
            error_type: None
        }));
    }

    #[test]
    fn automatic_claude_queries_wait_for_the_user_after_a_denied_keychain() {
        let now = Utc::now();
        let mut clock = RefreshClock::default();
        assert_eq!(claude_skip_reason(&clock, now, true), None);

        clock.claude_awaits_user = true;
        assert_eq!(
            claude_skip_reason(&clock, now, true),
            Some(NEEDS_AUTHORIZATION)
        );
        // A manual refresh still goes ahead.
        assert_eq!(claude_skip_reason(&clock, now, false), None);

        clock.claude_awaits_user = false;
        clock.last_claude = Some(now - chrono::Duration::seconds(30));
        assert_eq!(claude_skip_reason(&clock, now, true), Some("60 秒内已查询"));
        assert_eq!(
            claude_skip_reason(&clock, now, false),
            Some("60 秒内已查询")
        );

        clock.last_claude = Some(now - chrono::Duration::seconds(61));
        clock.claude_retry_at = Some(now + chrono::Duration::minutes(5));
        assert_eq!(claude_skip_reason(&clock, now, false), Some("限流退避中"));
    }
}
