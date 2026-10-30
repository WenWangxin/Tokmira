#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod bundle_rename;
mod identity;
mod login_item;
mod notifications;
mod quota_follow;
mod realtime;
mod session_open;
mod tray;
#[cfg(target_os = "macos")]
mod tray_image;
mod tray_menu;
mod update_text;
mod updates;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use token_audit_core::advice::{
    build_advice_lang, quota_advice_states, AdviceAlert, AdviceConfig, AdviceInput, AdviceReport,
    AlertTracker,
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
use token_audit_core::environment::scan_environment;
use token_audit_core::i18n::Lang;
use token_audit_core::model::{CompactEvent, PromptEvent, Tool};
use token_audit_core::pricing::Pricing;
use token_audit_core::quota_store::{self, CodexDaily};
use token_audit_core::recorder::{self, RecorderParts, RecorderPlan, RecorderStatus};
use token_audit_core::sessions::{
    list_sessions, session_detail, SessionDetail, SessionInput, SessionRow,
};
use token_audit_core::settings::{self, AppSettings};
use token_audit_core::share::{self, share_summary, ShareSummary};
use token_audit_core::timeline::build_timeline;
use token_audit_core::usage::{list_usage_windows, usage_detail_lang, UsageDetail, UsageWindowRef};

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

fn current_lang(data_dir: &std::path::Path) -> Lang {
    settings::read_settings(data_dir)
        .unwrap_or_default()
        .language
        .resolve(sys_locale::get_locale)
}

fn desktop_lang() -> Lang {
    dirs::data_dir()
        .map(|dir| current_lang(&dir.join(identity::LEGACY_NAME)))
        .unwrap_or_else(|| {
            token_audit_core::settings::Language::System.resolve(sys_locale::get_locale)
        })
}

fn display_text(zh: &'static str, en: &'static str) -> String {
    desktop_lang().pick(zh, en).to_owned()
}

fn config_error(error: &str) -> String {
    let lang = desktop_lang();
    match error {
        "内置价格表格式错误" => lang.pick(
            "内置价格表格式错误",
            "Bundled pricing table has an invalid format",
        ),
        "内置价格表价格无效" => lang.pick(
            "内置价格表价格无效",
            "Bundled pricing table contains invalid prices",
        ),
        "内置建议配置格式错误" => lang.pick(
            "内置建议配置格式错误",
            "Bundled advice configuration has an invalid format",
        ),
        _ => lang.pick(
            "配置文件格式错误",
            "Configuration file has an invalid format",
        ),
    }
    .to_owned()
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum QuotaState {
    Off,
    Updated,
    NeedsAuthorization,
    Backoff,
    Recent,
    Error,
}

#[derive(Serialize)]
struct QuotaOutcome {
    state: QuotaState,
    message: String,
}

impl QuotaOutcome {
    fn status(state: QuotaState) -> Self {
        Self {
            state,
            message: String::new(),
        }
    }

    fn error(message: String) -> Self {
        Self {
            state: QuotaState::Error,
            message,
        }
    }
}

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
) -> Option<QuotaState> {
    if automatic && clock.claude_awaits_user {
        return Some(QuotaState::NeedsAuthorization);
    }
    if clock.claude_retry_at.is_some_and(|at| now < at) {
        return Some(QuotaState::Backoff);
    }
    if clock
        .last_claude
        .is_some_and(|at| now - at < chrono::Duration::seconds(60))
    {
        return Some(QuotaState::Recent);
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
            .map_err(|_| display_text("扫描 Claude 日志失败", "Could not scan Claude logs"))?;
        let codex = self
            .codex
            .scan(home)
            .map_err(|_| display_text("扫描 Codex 日志失败", "Could not scan Codex logs"))?;
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
    display_text(
        "应用状态暂不可用",
        "Application state is temporarily unavailable",
    )
}
fn cli_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|_| {
        display_text(
            "无法定位桌面应用程序",
            "Could not locate the desktop application",
        )
    })?;
    let name = if cfg!(windows) {
        "token-audit.exe"
    } else {
        "token-audit"
    };
    let path = current
        .parent()
        .ok_or_else(|| {
            display_text(
                "无法定位桌面应用目录",
                "Could not locate the desktop application folder",
            )
        })?
        .join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(token_audit_core::tr!(
            desktop_lang(),
            "未找到随应用分发的命令行程序：{}",
            "Bundled CLI executable not found: {}",
            path.display()
        ))
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    settings::read_settings(&state.data_dir)
        .map_err(|_| display_text("读取设置失败", "Could not read settings"))
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
    // not bring the notice back. The update bookkeeping is likewise backend-owned.
    if let Some(current) = &current {
        settings.tray_hint_shown |= current.tray_hint_shown;
        settings.update_last_checked_at = current.update_last_checked_at;
        settings
            .update_notified_version
            .clone_from(&current.update_notified_version);
    }
    settings::normalize_menu_bar_tools(&mut settings.menu_bar_tools);
    settings::write_settings(&state.data_dir, &settings)
        .map_err(|_| display_text("保存设置失败", "Could not save settings"))?;
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
    .map_err(|_| display_text("扫描任务失败", "Scan task failed"))?
}
#[derive(Serialize)]
struct ScanSummary {
    events: usize,
    prompts: usize,
    malformed_lines: u64,
}

#[derive(Serialize)]
struct RefreshResult {
    claude: QuotaOutcome,
    codex: QuotaOutcome,
}
fn proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .or_else(system_proxy)
}
fn refresh_quotas_sync(state: &AppState, automatic: bool) -> Result<RefreshResult, String> {
    refresh_sync(state, automatic, true)
}
/// The automatic query after new Claude usage; Codex readings already arrive in its logs.
fn refresh_claude_sync(state: &AppState) -> Result<RefreshResult, String> {
    refresh_sync(state, true, false)
}
fn refresh_sync(
    state: &AppState,
    automatic: bool,
    include_codex: bool,
) -> Result<RefreshResult, String> {
    let lang = current_lang(&state.data_dir);
    let settings = settings::read_settings(&state.data_dir)
        .map_err(|_| display_text("读取设置失败", "Could not read settings"))?;
    let mut clock = state.refresh.lock().map_err(|_| lock_error())?;
    let now = Utc::now();
    let mut result = RefreshResult {
        claude: QuotaOutcome::status(QuotaState::Off),
        codex: QuotaOutcome::status(QuotaState::Off),
    };
    if settings.claude_official_query {
        if let Some(reason) = claude_skip_reason(&clock, now, automatic) {
            result.claude = QuotaOutcome::status(reason);
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
                    quota_store::append_claude(&state.data_dir, &(&quota).into()).map_err(
                        |_| {
                            display_text(
                                "保存 Claude 额度快照失败",
                                "Could not save Claude quota snapshot",
                            )
                        },
                    )?;
                    clock.claude_needs_authorization = false;
                    result.claude = QuotaOutcome::status(QuotaState::Updated);
                }
                Err(ClaudeQuotaError::RateLimited { retry_after }) => {
                    clock.claude_retry_at = Some(
                        now + chrono::Duration::from_std(
                            retry_after.unwrap_or(Duration::from_secs(300)),
                        )
                        .unwrap_or(chrono::Duration::minutes(5)),
                    );
                    result.claude = QuotaOutcome::status(QuotaState::Backoff);
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
                        QuotaOutcome::status(QuotaState::NeedsAuthorization)
                    } else {
                        QuotaOutcome::error(error.message(lang))
                    };
                    if result.claude.state == QuotaState::NeedsAuthorization {
                        clock.claude_needs_authorization = true;
                    }
                }
            }
        }
    }
    if include_codex && settings.codex_app_server {
        if clock
            .last_codex
            .is_some_and(|at| now - at < chrono::Duration::seconds(60))
        {
            result.codex = QuotaOutcome::status(QuotaState::Recent);
        } else {
            clock.last_codex = Some(now);
            result.codex = match codex_candidates(&state.home, std::env::var_os("PATH").as_deref())
                .into_iter()
                .next()
            {
                Some(path) => {
                    match read_codex_quota(&resolve_launch(&path), &AppServerOptions::default()) {
                        Ok(quota) => {
                            quota_store::append_codex(&state.data_dir, &(&quota).into()).map_err(
                                |_| {
                                    display_text(
                                        "保存 Codex 额度快照失败",
                                        "Could not save Codex quota snapshot",
                                    )
                                },
                            )?;
                            if let Some(usage) = quota.usage {
                                let days: Vec<_> = usage
                                    .daily
                                    .into_iter()
                                    .map(|d| CodexDaily {
                                        date: d.date,
                                        tokens: d.tokens,
                                    })
                                    .collect();
                                quota_store::write_codex_daily(&state.data_dir, &days).map_err(
                                    |_| {
                                        display_text(
                                            "保存 Codex 每日用量失败",
                                            "Could not save Codex daily usage",
                                        )
                                    },
                                )?;
                            }
                            QuotaOutcome::status(QuotaState::Updated)
                        }
                        Err(CodexQuotaError::Rpc { code, .. }) => {
                            QuotaOutcome::error(token_audit_core::tr!(
                                lang,
                                "Codex app-server 错误代码 {code}",
                                "Codex app-server error code {code}"
                            ))
                        }
                        Err(error) => QuotaOutcome::error(error.message(lang)),
                    }
                }
                None => QuotaOutcome::error(
                    lang.pick("未找到 Codex 命令行程序", "Codex CLI executable not found")
                        .to_owned(),
                ),
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
    .map_err(|_| display_text("额度查询任务失败", "Quota query task failed"))?;
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
        .map_err(|_| display_text("生成仪表盘失败", "Could not build the dashboard"))?
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
        let settings = settings::read_settings(&data_dir)
            .map_err(|_| display_text("读取设置失败", "Could not read settings"))?;
        let points = build_timeline(
            &claude_snapshots,
            &codex_snapshots,
            &statusline,
            &scans.codex.rate_limits,
            settings.calibration.as_ref(),
        );
        let pricing = Pricing::bundled().map_err(config_error)?;
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
    .map_err(|_| display_text("生成额度归因失败", "Could not attribute quota usage"))?
}

/// Everything a page command needs, computed once from the cached scan.
struct UsageContext<'a> {
    home: &'a std::path::Path,
    input: &'a OverviewInput<'a>,
    prompts: &'a [PromptEvent],
    compactions: &'a [CompactEvent],
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
    .map_err(|_| display_text("生成分享摘要失败", "Could not build the share summary"))?
}

#[tauri::command]
async fn save_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    let lang = current_lang(&state.data_dir);
    let directory = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| display_text("无法确定下载目录", "Could not locate the Downloads folder"))?;
    let is_png = file_name.to_ascii_lowercase().ends_with(".png");
    let title = display_text("另存为", "Save As");
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app
            .dialog()
            .file()
            .set_title(title)
            .set_directory(directory)
            .set_file_name(&file_name);
        let dialog = if let Some(window) = app.get_webview_window("main") {
            dialog.set_parent(&window)
        } else {
            dialog
        };
        let dialog = if is_png {
            dialog.add_filter(display_text("PNG 图片", "PNG image"), &["png"])
        } else {
            dialog.add_filter(display_text("Markdown 文档", "Markdown document"), &["md"])
        };
        let Some(selected) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|_| display_text("无法使用所选路径", "Could not use the selected path"))?;
        share::save_export_path_lang(&path, &file_name, &bytes, lang)
            .map(|saved| Some(saved.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| display_text("保存文件失败", "Could not save the file"))?
}

/// Brings a session back to the front and, with `command`, types it for the user to send.
#[tauri::command]
async fn open_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    target: session_open::Target,
    command: Option<String>,
) -> Result<token_audit_core::session_open::OpenOutcome, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_open::open(&app, &data_dir, &target, command.as_deref())
    })
    .await
    .map_err(|_| display_text("打开会话失败", "Could not open the session"))
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
    .map_err(|_| display_text("检测本机工具失败", "Could not detect local tools"))?
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
    let settings = settings::read_settings(data_dir)
        .map_err(|_| display_text("读取设置失败", "Could not read settings"))?;
    let points = build_timeline(
        &claude_snapshots,
        &codex_snapshots,
        &statusline,
        &scans.codex.rate_limits,
        settings.calibration.as_ref(),
    );
    let pricing = Pricing::bundled().map_err(config_error)?;
    let mut compactions = scans.claude.compactions;
    compactions.extend(scans.codex.compactions);
    let mut events = scans.claude.events;
    events.extend(scans.codex.events);
    let now = Utc::now();
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
        home,
        input: &input,
        prompts: &prompts,
        compactions: &compactions,
        settings: &settings,
        now,
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
    .map_err(|_| display_text("生成用量窗口失败", "Could not build usage windows"))?
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
                .ok_or_else(|| {
                    display_text(
                        "指定的用量窗口不存在",
                        "The selected usage window does not exist",
                    )
                })?;
            Ok(usage_detail_lang(
                input,
                &window,
                now,
                current_lang(&data_dir),
            ))
        })
    })
    .await
    .map_err(|_| display_text("生成用量明细失败", "Could not build usage details"))?
}
#[tauri::command]
fn recorder_status(state: State<'_, AppState>) -> RecorderStatus {
    recorder::status(&state.home, &state.data_dir)
}
#[tauri::command]
fn recorder_plan(state: State<'_, AppState>, parts: RecorderParts) -> Result<RecorderPlan, String> {
    let exe = cli_executable()?;
    recorder::plan_install(&state.home, &state.data_dir, &exe, parts)
        .map_err(|error| error.message(current_lang(&state.data_dir)))
}
#[tauri::command]
fn recorder_install(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    let exe = cli_executable()?;
    recorder::install(&state.home, &state.data_dir, &exe, parts)
        .map_err(|error| error.message(current_lang(&state.data_dir)))
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
    let pricing = Pricing::bundled().map_err(config_error)?;
    let advice = AdviceConfig::bundled().map_err(config_error)?;
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
        pricing_note: if current_lang(&state.data_dir) == Lang::En {
            pricing.note_en
        } else {
            pricing.note
        },
        advice_version: advice.version,
        backup_dir: state.data_dir.join("recorder").join("backups"),
    })
}
#[tauri::command]
fn clear_recorder_records(state: State<'_, AppState>) -> Result<(), String> {
    recorder::clear_records(&state.data_dir)
        .map_err(|error| error.message(current_lang(&state.data_dir)))
}
#[tauri::command]
fn recorder_plan_uninstall(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    recorder::plan_uninstall(&state.home, &state.data_dir, parts)
        .map_err(|error| error.message(current_lang(&state.data_dir)))
}
#[tauri::command]
fn recorder_uninstall(
    state: State<'_, AppState>,
    parts: RecorderParts,
) -> Result<RecorderPlan, String> {
    recorder::uninstall(&state.home, &state.data_dir, parts)
        .map_err(|error| error.message(current_lang(&state.data_dir)))
}

#[tauri::command]
async fn get_advice(state: State<'_, AppState>, period_days: u32) -> Result<AdviceReport, String> {
    if period_days != 7 && period_days != 30 {
        return Err(display_text(
            "统计天数只能是 7 或 30",
            "The period must be 7 or 30 days",
        ));
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
    .map_err(|_| display_text("生成省额度建议失败", "Could not build savings tips"))?
}

fn session_input<'a>(context: &'a UsageContext<'a>) -> SessionInput<'a> {
    SessionInput {
        events: &context.input.attribution.events,
        titles: context.input.titles,
        pricing: context.input.pricing,
    }
}

fn advice_report(context: &UsageContext<'_>, period_days: u32) -> Result<AdviceReport, String> {
    let config = AdviceConfig::bundled().map_err(config_error)?;
    let projects: Vec<PathBuf> = context
        .input
        .events
        .iter()
        .filter_map(|event| event.project.as_ref().map(PathBuf::from))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let environment = scan_environment(context.home, &projects);
    let quotas = quota_advice_states(
        context.input.points,
        context.input.claude_snapshots,
        context.input.codex_snapshots,
        context.input.events,
        context.now,
        config.plan_fit.max_end_gap_hours,
    );
    Ok(build_advice_lang(
        &AdviceInput {
            events: &context.input.attribution.events,
            prompts: context.prompts,
            pricing: context.input.pricing,
            config: &config,
            dismissed: &context.settings.dismissed_advice,
            compactions: context.compactions,
            environment: Some(&environment),
            quotas: &quotas,
        },
        period_days,
        context.now,
        context.settings.language.resolve(sys_locale::get_locale),
    ))
}

#[tauri::command]
async fn get_sessions(state: State<'_, AppState>, days: u32) -> Result<Vec<SessionRow>, String> {
    if days != 7 && days != 30 {
        return Err(display_text(
            "统计天数只能是 7 或 30",
            "The period must be 7 or 30 days",
        ));
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
    .map_err(|_| display_text("生成会话列表失败", "Could not build the session list"))?
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
            let config = AdviceConfig::bundled().map_err(config_error)?;
            let tags = build_advice_lang(
                &AdviceInput {
                    events: &context.input.attribution.events,
                    prompts: context.prompts,
                    pricing: context.input.pricing,
                    config: &config,
                    dismissed: &context.settings.dismissed_advice,
                    compactions: context.compactions,
                    environment: None,
                    quotas: &[],
                },
                30,
                context.now,
                context.settings.language.resolve(sys_locale::get_locale),
            )
            .event_tags;
            session_detail(
                &session_input(context),
                context.prompts,
                &tags,
                tool,
                &session_id,
            )
            .ok_or_else(|| display_text("未找到该会话", "Session not found"))
        })
    })
    .await
    .map_err(|_| display_text("生成会话详情失败", "Could not build session details"))?
}

fn set_dismissed(state: &AppState, key: &str, dismissed: bool) -> Result<AppSettings, String> {
    let mut settings = settings::read_settings(&state.data_dir)
        .map_err(|_| display_text("读取设置失败", "Could not read settings"))?;
    settings.dismissed_advice.retain(|item| item != key);
    if dismissed {
        settings.dismissed_advice.push(key.to_owned());
    }
    settings::write_settings(&state.data_dir, &settings)
        .map_err(|_| display_text("保存设置失败", "Could not save settings"))?;
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
    /// New Claude log lines; drives the follow-up official query.
    claude_usage: mpsc::Sender<()>,
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
        let claude_logs = change.claude_logs;
        let _ = self.app.emit("usage-changed", change);
        let _ = self.alerts.send(());
        if claude_logs {
            let _ = self.claude_usage.send(());
        }
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
                    compactions: context.compactions,
                    environment: None,
                    quotas: &[],
                };
                if seed {
                    tracker.seed(&input, context.now);
                    return Ok(None);
                }
                // Checked even when switched off, so turning it on later does not report old problems.
                let alert = tracker.check_lang(
                    &input,
                    context.now,
                    context.settings.language.resolve(sys_locale::get_locale),
                );
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
        let payload = serde_json::to_string(&session_open::ActionPayload::for_alert(alert))
            .unwrap_or_default();
        notifications::post_alert(
            app,
            &alert.title,
            &alert.body,
            &payload,
            alert.command.is_some(),
        );
    }
}

/// A notification button: brings the session forward and, for "fill", types the command.
/// Runs off the notification callback; a follow-up notice says what happened when the
/// result is not on screen or the clipboard changed.
fn notification_action(app: &tauri::AppHandle, payload: String, fill: bool) {
    use tauri::Manager;
    let Ok(payload) = serde_json::from_str::<session_open::ActionPayload>(&payload) else {
        tray::show_main(app);
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let data_dir = app.state::<AppState>().data_dir.clone();
        let command = payload.command.filter(|_| fill);
        let outcome = session_open::open(&app, &data_dir, &payload.target, command.as_deref());
        if let Some(text) = token_audit_core::session_open::outcome_notice(
            outcome,
            command.as_deref(),
            current_lang(&data_dir),
        ) {
            notifications::post(&app, "Tokmira", &text);
        }
    });
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

#[cfg(target_os = "macos")]
fn open_moved_bundle(app: &tauri::App, bundle: &std::path::Path) -> std::io::Result<()> {
    // The macOS single-instance plugin uses the bundle identifier for its
    // socket. Release it before `open -n` so the new process can start.
    app.handle().remove_plugin("single-instance");
    tauri_plugin_single_instance::destroy(app.handle());
    let result = std::process::Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .status()
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "open could not launch the renamed app",
                ))
            }
        });
    if result.is_err() {
        let _ = app
            .handle()
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                tray::show_main(app)
            }));
    }
    result
}

#[cfg(target_os = "macos")]
fn handle_bundle_rename(
    app: &tauri::App,
    home: &std::path::Path,
    data_dir: &std::path::Path,
) -> bool {
    use objc2_foundation::NSBundle;

    let bundle = NSBundle::mainBundle();
    let path = PathBuf::from(bundle.bundlePath().to_string());
    let identifier = bundle
        .bundleIdentifier()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let marker = bundle_rename::marker(data_dir);
    if path.file_name().is_some_and(|name| name == "Tokmira.app") {
        if std::fs::read(&marker).is_ok_and(|bytes| bytes == b"enabled\n") {
            // One attempt only. Renaming back on failure would repeat the whole
            // migration on every launch; the user can switch autostart on again.
            let _ = login_item::refresh_after_bundle_move(app.handle());
        }
        let _ = std::fs::remove_file(&marker);
        return false;
    }

    if bundle_rename::should_migrate(&path, &identifier, !tauri::is_dev())
        != bundle_rename::Decision::Migrate
    {
        return false;
    }
    let status = recorder::status(home, data_dir);
    let parts = RecorderParts {
        statusline: status.statusline_installed,
        hooks: status.hooks_installed,
    };
    let autostart = login_item::get(app.handle()).enabled;
    if bundle_rename::execute(&path, home, data_dir, parts, autostart, |new| {
        open_moved_bundle(app, new)
    })
    .is_ok_and(|launched| launched)
    {
        app.handle().exit(0);
        return true;
    }
    false
}

fn main() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(identity::LEGACY_NAME)
                .build(),
        )
        .setup(|app| {
            use tauri::Manager;
            let home = dirs::home_dir().ok_or_else(|| {
                std::io::Error::other(display_text(
                    "无法确定用户主目录",
                    "Could not locate the home folder",
                ))
            })?;
            let data_dir = dirs::data_dir()
                .ok_or_else(|| {
                    std::io::Error::other(display_text(
                        "无法确定应用数据目录",
                        "Could not locate the application data folder",
                    ))
                })?
                .join(identity::LEGACY_NAME);
            #[cfg(target_os = "macos")]
            if handle_bundle_rename(app, &home, &data_dir) {
                return Ok(());
            }
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
                    claude_usage: quota_follow::spawn(app.handle().clone()),
                },
            );
            app.manage(updates::UpdateState::default());
            updates::spawn_auto_check(app.handle().clone(), data_dir.clone());
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
            // Dev builds share the installed app's settings; a silent start there would leave
            // `tauri dev` running with no window and no Dock icon.
            if launch_settings.onboarding_completed
                && launch_settings.silent_start
                && !tauri::is_dev()
            {
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
            open_session,
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
            open_login_items_settings,
            updates::get_update_status,
            updates::check_update,
            updates::install_update,
            updates::restart_app
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
            Some(QuotaState::NeedsAuthorization)
        );
        // A manual refresh still goes ahead.
        assert_eq!(claude_skip_reason(&clock, now, false), None);

        clock.claude_awaits_user = false;
        clock.last_claude = Some(now - chrono::Duration::seconds(30));
        assert_eq!(
            claude_skip_reason(&clock, now, true),
            Some(QuotaState::Recent)
        );
        assert_eq!(
            claude_skip_reason(&clock, now, false),
            Some(QuotaState::Recent)
        );

        clock.last_claude = Some(now - chrono::Duration::seconds(61));
        clock.claude_retry_at = Some(now + chrono::Duration::minutes(5));
        assert_eq!(
            claude_skip_reason(&clock, now, false),
            Some(QuotaState::Backoff)
        );
    }
}
