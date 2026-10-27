//! Checking for, downloading and installing new versions. Only the fixed GitHub manifest in
//! `tauri.conf.json` is requested, and only when the user clicks "检查更新" or has turned on
//! the automatic check. The plugin verifies every package against the bundled public key.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use token_audit_core::settings;
use token_audit_core::update::{auto_check_due, should_notify};

use crate::update_text::{info, message, UpdateInfo};
use crate::{current_lang, desktop_lang, notifications, tray, AppState};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct UpdateState {
    /// The update the last check found; installing uses exactly this one.
    pending: Mutex<Option<Update>>,
    installing: AtomicBool,
    /// Set once the new version is in place and only a restart is left.
    installed: AtomicBool,
}

#[derive(Debug, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub available: Option<UpdateInfo>,
    pub installed: bool,
}

#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// The update found by the last check, for the settings page and the tray menu.
pub fn available(app: &AppHandle) -> Option<UpdateInfo> {
    let state = app.try_state::<UpdateState>()?;
    let pending = state.pending.lock().ok()?;
    pending.as_ref().map(info)
}

async fn check(app: &AppHandle, data_dir: &Path) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater_builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| message(&error, current_lang(data_dir)))?;
    // Recorded before the request, so a failing network still counts toward the daily limit.
    if let Ok(mut current) = settings::read_settings(data_dir) {
        current.update_last_checked_at = Some(Utc::now());
        let _ = settings::write_settings(data_dir, &current);
    }
    let found = updater
        .check()
        .await
        .map_err(|error| message(&error, current_lang(data_dir)))?;
    let found_info = found.as_ref().map(info);
    if let Ok(mut pending) = app.state::<UpdateState>().pending.lock() {
        *pending = found;
    }
    tray::signal(app);
    Ok(found_info)
}

#[tauri::command]
pub fn get_update_status(app: AppHandle, state: State<'_, AppState>) -> UpdateStatus {
    UpdateStatus {
        current_version: app.package_info().version.to_string(),
        last_checked_at: settings::read_settings(&state.data_dir)
            .ok()
            .and_then(|current| current.update_last_checked_at),
        available: available(&app),
        installed: app.state::<UpdateState>().installed.load(Ordering::SeqCst),
    }
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateStatus, String> {
    check(&app, &state.data_dir).await?;
    Ok(get_update_status(app, state))
}

/// Downloads and verifies the update the last check found, then installs it. On Windows
/// the installer takes over and the app exits; elsewhere the caller asks for a restart.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let update = state
        .pending
        .lock()
        .map_err(|_| {
            desktop_lang()
                .pick("更新状态不可用", "Update state is unavailable")
                .to_owned()
        })?
        .clone()
        .ok_or_else(|| {
            desktop_lang()
                .pick(
                    "没有可安装的更新，请先检查更新",
                    "No update is ready to install. Check for updates first.",
                )
                .to_owned()
        })?;
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err(desktop_lang()
            .pick("正在安装更新", "An update is already being installed")
            .to_owned());
    }
    let mut downloaded = 0_u64;
    let progress = app.clone();
    let result = update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await;
    state.installing.store(false, Ordering::SeqCst);
    result.map_err(|error| message(&error, desktop_lang()))?;
    state.installed.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.request_restart();
}

/// The background check: at most once a day, only while the user has it switched on.
/// The first look waits a minute so launching the app never starts with a request.
pub fn spawn_auto_check(app: AppHandle, data_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        // Also kept in memory, so a settings file that cannot be written does not mean hourly requests.
        let mut last_attempt = None;
        loop {
            if let Ok(current) = settings::read_settings(&data_dir) {
                let now = Utc::now();
                if auto_check_due(
                    current.auto_check_updates,
                    current.update_last_checked_at.max(last_attempt),
                    now,
                ) {
                    last_attempt = Some(now);
                    let found = tauri::async_runtime::block_on(check(&app, &data_dir));
                    if let Ok(Some(found)) = found {
                        announce(&app, &data_dir, &found);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

fn announce(app: &AppHandle, data_dir: &Path, found: &UpdateInfo) {
    let Ok(mut current) = settings::read_settings(data_dir) else {
        return;
    };
    if !should_notify(&found.version, current.update_notified_version.as_deref()) {
        return;
    }
    current.update_notified_version = Some(found.version.clone());
    if settings::write_settings(data_dir, &current).is_err() {
        return;
    }
    let focused = app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
    });
    if focused {
        let _ = app.emit("update-available", found);
    } else {
        let lang = current_lang(data_dir);
        notifications::post(
            app,
            &token_audit_core::tr!(
                lang,
                "token-audit {} 可以更新了",
                "token-audit {} is available",
                found.version
            ),
            lang.pick(
                "打开 token-audit，在“设置 → 关于与更新”中查看更新说明并安装。",
                "Open token-audit and go to Settings → About & updates to review and install it.",
            ),
        );
    }
}
