//! DeepSeek Harness in the desktop app: yuan costs on the shared pages, its own usage
//! and advice views, and the balance query the user can turn on.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::State;
use token_audit_core::dashboard::Overview;
use token_audit_core::deepseek::{deepseek_root, DeepSeekScan};
use token_audit_core::deepseek_balance::{
    query_deepseek_balance, BalanceError, BalanceOptions, ClientIdentity, UreqBalanceTransport,
};
use token_audit_core::deepseek_view::{
    deepseek_advice, deepseek_overview, deepseek_usage, DeepSeekAdvice, DeepSeekBalance,
    DeepSeekCosting, DeepSeekPeriod, DeepSeekUsage,
};
use token_audit_core::detect::{detect_deepseek_install, DeepSeekPlatform, DetectedTool};
use token_audit_core::i18n::Lang;
use token_audit_core::model::{Source, Tool};
use token_audit_core::quota_store;
use token_audit_core::sessions::{SessionDetail, SessionRow};
use token_audit_core::settings::AppSettings;

use crate::{display_text, lock_error, with_usage, AppState, QuotaOutcome, QuotaState};

/// DeepSeek's top-up page, the address Harness itself links to.
const TOP_UP_URL: &str = "https://platform.deepseek.com/top_up";

/// A background poll waits this long after the last query; the wallet changes slowly.
const AUTOMATIC_INTERVAL: chrono::Duration = chrono::Duration::minutes(10);
/// Even a manual refresh waits this long, to keep requests to DeepSeek rare.
const MANUAL_INTERVAL: chrono::Duration = chrono::Duration::seconds(60);

/// `$DSH_HOME`, or `~/.dsh`.
pub fn root(home: &Path) -> PathBuf {
    deepseek_root(home, std::env::var_os("DSH_HOME").as_deref())
}

/// The stored official reading while the query is on; otherwise the typed-in
/// balance less the spend estimated since.
pub fn current_balance(
    data_dir: &Path,
    settings: &AppSettings,
    costing: &DeepSeekCosting<'_>,
    now: DateTime<Utc>,
) -> Option<DeepSeekBalance> {
    if settings.deepseek_balance_query {
        quota_store::read_deepseek_balance(data_dir)
    } else {
        settings
            .deepseek_manual_balance
            .as_ref()
            .map(|manual| costing.manual_balance(manual, now))
    }
}

/// Adds the balance card and the yuan cost of each DeepSeek session.
pub fn fill_overview(
    overview: &mut Overview,
    costing: &DeepSeekCosting<'_>,
    scan: &DeepSeekScan,
    data_dir: &Path,
    settings: &AppSettings,
    now: DateTime<Utc>,
) {
    for session in &mut overview.recent_sessions {
        if session.tool == Tool::DeepSeek {
            session.estimated_cost_cny =
                costing.session_cost(&session.session_id, session.first_at, session.last_at);
        }
    }
    let balance = current_balance(data_dir, settings, costing, now);
    if !scan.events.is_empty() || balance.is_some() || settings.deepseek_balance_query {
        overview.deepseek = Some(deepseek_overview(costing, balance, now, &chrono::Local));
    }
}

pub fn fill_rows(rows: &mut [SessionRow], costing: &DeepSeekCosting<'_>) {
    for row in rows.iter_mut().filter(|row| row.tool == Tool::DeepSeek) {
        row.estimated_cost_cny = costing.session_cost(&row.session_id, row.first_at, row.last_at);
    }
}

pub fn fill_detail(detail: &mut SessionDetail, costing: &DeepSeekCosting<'_>) {
    if detail.tool == Tool::DeepSeek {
        detail.estimated_cost_cny =
            costing.session_cost(&detail.session_id, detail.first_at, detail.last_at);
    }
}

/// The onboarding row: sessions from the logs, and which Harness apps are installed.
pub fn detected(scan: &DeepSeekScan, home: &Path) -> DetectedTool {
    let root = root(home);
    let platform = if cfg!(windows) {
        DeepSeekPlatform::Windows
    } else {
        DeepSeekPlatform::Mac
    };
    let install = detect_deepseek_install(
        &root,
        home,
        Path::new("/Applications/DeepSeek Harness.app"),
        Path::new("/usr/local/bin/dsh"),
        std::env::var_os("PATH").as_deref(),
        platform,
    );
    DetectedTool {
        source: Source::DeepSeek,
        sessions: scan
            .events
            .iter()
            .map(|event| event.session_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        requests: scan.events.len(),
        last_used_at: scan.events.iter().map(|event| event.timestamp).max(),
        plan: None,
        install: Some(install),
    }
}

/// Queries the balance when the user turned it on and the last query is old enough.
/// A failure keeps the stored reading, so a timeout never shows a zero balance.
pub fn refresh(
    state: &AppState,
    settings: &AppSettings,
    last: &mut Option<DateTime<Utc>>,
    automatic: bool,
    lang: Lang,
) -> QuotaOutcome {
    if !settings.deepseek_balance_query {
        return QuotaOutcome::status(QuotaState::Off);
    }
    let now = Utc::now();
    let interval = if automatic {
        AUTOMATIC_INTERVAL
    } else {
        MANUAL_INTERVAL
    };
    if last.is_some_and(|at| now - at < interval) {
        return QuotaOutcome::status(QuotaState::Recent);
    }
    let previous = *last;
    *last = Some(now);
    let options = BalanceOptions {
        credential: settings.deepseek_credential,
        proxy: crate::proxy(),
        timeout: Duration::from_secs(15),
        client: ClientIdentity {
            windows: cfg!(windows),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            chinese: lang == Lang::Zh,
            utc_offset_seconds: chrono::Local::now().offset().local_minus_utc(),
        },
    };
    match query_deepseek_balance(
        &root(&state.home),
        std::env::var_os("DEEPSEEK_API_KEY").as_deref(),
        &UreqBalanceTransport,
        &options,
        now,
    ) {
        Ok(balance) => match quota_store::write_deepseek_balance(&state.data_dir, &balance) {
            Ok(()) => QuotaOutcome::status(QuotaState::Updated),
            Err(_) => QuotaOutcome::error(
                lang.pick(
                    "保存 DeepSeek 余额失败",
                    "Could not save the DeepSeek balance",
                )
                .to_owned(),
            ),
        },
        Err(error) => {
            // Nothing was sent without a login, so it does not count toward the interval.
            if error == BalanceError::NoCredential {
                *last = previous;
            }
            QuotaOutcome::error(error.message(lang))
        }
    }
}

/// Queries only the DeepSeek balance, for its settings switch: turning it on must not
/// also start the Claude or Codex queries.
#[tauri::command]
pub async fn refresh_deepseek_balance(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<QuotaOutcome, String> {
    let state = state.inner().clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let lang = crate::current_lang(&state.data_dir);
        let settings = token_audit_core::settings::read_settings(&state.data_dir)
            .map_err(|_| display_text("读取设置失败", "Could not read settings"))?;
        let mut clock = state.refresh.lock().map_err(|_| lock_error())?;
        Ok::<_, String>(refresh(
            &state,
            &settings,
            &mut clock.last_deepseek,
            false,
            lang,
        ))
    })
    .await
    .map_err(|_| display_text("余额查询任务失败", "Balance query task failed"))??;
    crate::tray::signal(&app);
    Ok(outcome)
}

/// Opens the top-up page in the default browser. The address is fixed here, so the
/// page cannot pass in another one.
#[tauri::command]
pub fn open_deepseek_top_up() -> Result<(), String> {
    if crate::session_open::open_url(TOP_UP_URL) {
        Ok(())
    } else {
        Err(display_text("无法打开浏览器", "Could not open the browser"))
    }
}

#[tauri::command]
pub async fn get_deepseek_usage(
    state: State<'_, AppState>,
    period: DeepSeekPeriod,
) -> Result<DeepSeekUsage, String> {
    let home = state.home.clone();
    let data_dir = state.data_dir.clone();
    let cached = Arc::clone(&state.scans);
    tauri::async_runtime::spawn_blocking(move || {
        with_usage(&home, &data_dir, &cached, |context| {
            let costing = DeepSeekCosting::new(context.deepseek, &context.input.pricing.deepseek);
            let balance = current_balance(&data_dir, context.settings, &costing, context.now);
            Ok(deepseek_usage(
                &costing,
                period,
                balance,
                context.now,
                &chrono::Local,
                crate::current_lang(&data_dir),
            ))
        })
    })
    .await
    .map_err(|_| display_text("生成 DeepSeek 用量失败", "Could not build DeepSeek usage"))?
}

#[tauri::command]
pub async fn get_deepseek_advice(
    state: State<'_, AppState>,
    period_days: u32,
) -> Result<DeepSeekAdvice, String> {
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
            let costing = DeepSeekCosting::new(context.deepseek, &context.input.pricing.deepseek);
            Ok(deepseek_advice(
                &costing,
                period_days,
                &context.settings.dismissed_advice,
                context.now,
            ))
        })
    })
    .await
    .map_err(|_| lock_error())?
}
