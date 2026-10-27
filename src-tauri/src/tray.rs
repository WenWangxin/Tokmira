//! The persistent menu bar / notification area icon and its live quota menu.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
#[cfg(target_os = "macos")]
use tauri::menu::Submenu;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager};
use token_audit_core::model::Tool;
use token_audit_core::settings::{self, MenuBarQuota};

use crate::{login_item, tray_menu, AppState};

struct TrayState {
    sender: Sender<()>,
    tray: TrayIcon,
    lines: Vec<MenuItem<tauri::Wry>>,
    authorization: MenuItem<tauri::Wry>,
    updated: MenuItem<tauri::Wry>,
    refresh: MenuItem<tauri::Wry>,
    dashboard: MenuItem<tauri::Wry>,
    advice: MenuItem<tauri::Wry>,
    alerts: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
    silent: CheckMenuItem<tauri::Wry>,
    modes: Vec<CheckMenuItem<tauri::Wry>>,
    #[cfg(target_os = "macos")]
    mode_submenu: Submenu<tauri::Wry>,
    update: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    /// Quota lines shown, authorization row shown, update row shown.
    layout: Mutex<(usize, bool, bool)>,
}

pub fn signal(app: &AppHandle) {
    if let Some(state) = app.try_state::<TrayState>() {
        let _ = state.sender.send(());
    }
}

fn navigate(app: &AppHandle, path: &str) {
    show_main(app);
    let _ = app.emit("navigate", path);
}

fn edit_settings(app: &AppHandle, change: impl FnOnce(&mut settings::AppSettings)) {
    let state = app.state::<AppState>();
    if let Ok(mut current) = settings::read_settings(&state.data_dir) {
        change(&mut current);
        if settings::write_settings(&state.data_dir, &current).is_ok() {
            signal(app);
            let _ = app.emit("settings-changed", ());
        }
    }
}

fn menu_event(app: &AppHandle, id: &str) {
    match id {
        "refresh" => {
            let app = app.clone();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                if let Ok(mut scans) = state.scans.lock() {
                    let _ = scans.refresh(&state.home);
                }
                let _ = crate::refresh_quotas_sync(&state, true);
                signal(&app);
                let _ = app.emit(
                    "usage-changed",
                    crate::realtime::UsageChanged {
                        tools: vec!["Claude", "Codex"],
                    },
                );
            });
        }
        "dashboard" | "authorization" => navigate(app, "/"),
        "advice" => navigate(app, "/advice"),
        "settings" | "update" => navigate(app, "/settings"),
        "alerts" => edit_settings(app, |s| s.advice_notifications = !s.advice_notifications),
        "silent" => edit_settings(app, |s| s.silent_start = !s.silent_start),
        "autostart" => {
            let enabled = !login_item::get(app).enabled;
            if login_item::set(app, enabled).is_ok() {
                signal(app);
                let _ = app.emit("settings-changed", ());
            }
        }
        "quota-off" => edit_settings(app, |s| s.menu_bar_quota = MenuBarQuota::Off),
        "quota-claude" => edit_settings(app, |s| s.menu_bar_quota = MenuBarQuota::Claude),
        "quota-codex" => edit_settings(app, |s| s.menu_bar_quota = MenuBarQuota::Codex),
        "quota-both" => edit_settings(app, |s| s.menu_bar_quota = MenuBarQuota::Both),
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let mut lines = Vec::new();
    for i in 0..6 {
        lines.push(MenuItem::with_id(
            app,
            format!("info-{i}"),
            "",
            false,
            None::<&str>,
        )?);
    }
    let authorization = MenuItem::with_id(
        app,
        "authorization",
        "Claude 需要授权，点此打开应用",
        false,
        None::<&str>,
    )?;
    let updated = MenuItem::with_id(app, "updated", "更新于 --:--", false, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
    let dashboard = MenuItem::with_id(app, "dashboard", "打开仪表盘", true, None::<&str>)?;
    let advice = MenuItem::with_id(app, "advice", "省额度建议", true, None::<&str>)?;
    let alerts = CheckMenuItem::with_id(app, "alerts", "省额度提醒", true, false, None::<&str>)?;
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "开机自动启动", true, false, None::<&str>)?;
    let silent = CheckMenuItem::with_id(app, "silent", "静默启动", true, false, None::<&str>)?;
    let modes = [
        ("quota-off", "只显示图标"),
        ("quota-claude", "Claude"),
        ("quota-codex", "Codex"),
        ("quota-both", "两者都显示"),
    ]
    .into_iter()
    .map(|(id, label)| CheckMenuItem::with_id(app, id, label, true, false, None::<&str>))
    .collect::<tauri::Result<Vec<_>>>()?;
    let update_item = MenuItem::with_id(app, "update", "", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 token-audit", true, None::<&str>)?;
    let menu = Menu::new(app)?;
    for line in &lines {
        menu.append(line)?;
    }
    menu.append(&authorization)?;
    menu.append(&updated)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for item in [&refresh, &dashboard, &advice] {
        menu.append(item)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for item in [&alerts, &autostart, &silent] {
        menu.append(item)?;
    }
    #[cfg(target_os = "macos")]
    let mode_submenu = {
        let submenu = Submenu::with_id(app, "quota-menu", "菜单栏显示", true)?;
        for mode in &modes {
            submenu.append(mode)?;
        }
        menu.append(&submenu)?;
        submenu
    };
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&settings)?;
    menu.append(&quit)?;
    let tray = TrayIconBuilder::with_id("main")
        .tooltip("token-audit")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| menu_event(app, event.id().as_ref()));
    #[cfg(target_os = "macos")]
    let tray = tray
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray-template.png"
        ))?)
        .icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let tray = {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        let tray = tray.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
        match app.default_window_icon() {
            Some(icon) => tray.icon(icon.clone()),
            None => tray,
        }
    };
    let tray = tray.build(app)?;
    let (sender, receiver) = mpsc::channel::<()>();
    app.manage(TrayState {
        sender,
        tray,
        lines,
        authorization,
        updated,
        refresh,
        dashboard,
        advice,
        alerts,
        autostart,
        silent,
        modes,
        #[cfg(target_os = "macos")]
        mode_submenu,
        update: update_item,
        settings,
        quit,
        layout: Mutex::new((6, true, false)),
    });
    let app = app.handle().clone();
    signal(&app);
    std::thread::spawn(move || {
        // A silent start has no window polling, so the first hidden poll runs right away.
        let mut last_hidden_poll: Option<Instant> = None;
        loop {
            match receiver.recv_timeout(Duration::from_secs(60)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            while receiver.try_recv().is_ok() {}
            let hidden = app.get_webview_window("main").is_some_and(|window| {
                !window.is_visible().unwrap_or(true) || window.is_minimized().unwrap_or(false)
            });
            if hidden && last_hidden_poll.is_none_or(|at| at.elapsed() >= Duration::from_secs(300))
            {
                last_hidden_poll = Some(Instant::now());
                let _ = crate::refresh_quotas_sync(&app.state::<AppState>(), true);
            }
            update(&app);
        }
    });
    Ok(())
}

fn rebuild_menu(
    app: &AppHandle,
    state: &TrayState,
    count: usize,
    authorization: bool,
    update: bool,
) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    for line in state.lines.iter().take(count) {
        menu.append(line)?;
    }
    if authorization {
        menu.append(&state.authorization)?;
    }
    menu.append(&state.updated)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for item in [&state.refresh, &state.dashboard, &state.advice] {
        menu.append(item)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for item in [&state.alerts, &state.autostart, &state.silent] {
        menu.append(item)?;
    }
    #[cfg(target_os = "macos")]
    menu.append(&state.mode_submenu)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if update {
        menu.append(&state.update)?;
    }
    menu.append(&state.settings)?;
    menu.append(&state.quit)?;
    state.tray.set_menu(Some(menu))
}

fn update(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(settings) = settings::read_settings(&state.data_dir) else {
        return;
    };
    let Ok(overview) = crate::overview_sync(&state) else {
        return;
    };
    let needs_auth = state
        .refresh
        .lock()
        .ok()
        .is_some_and(|clock| clock.claude_needs_authorization);
    let claude = overview.tools.iter().find(|tool| tool.tool == Tool::Claude);
    let codex = overview.tools.iter().find(|tool| tool.tool == Tool::Codex);
    let claude_on = settings.claude_official_query
        || overview.recorder.statusline_installed
        || settings
            .calibration
            .as_ref()
            .is_some_and(|value| value.tool == Tool::Claude)
        || claude.is_some_and(|tool| tool.five_hour.used_percent.is_some());
    let codex_on = settings.codex_app_server
        || codex.is_some_and(|tool| tool.five_hour.used_percent.is_some());
    let (statusline, _) = token_audit_core::recorder::read_statusline_records(&state.data_dir);
    let statusline_at = statusline.iter().map(|record| record.recorded_at).max();
    let updated_at = [
        overview.quota_sources.claude_official,
        overview.quota_sources.codex_official,
        overview.quota_sources.codex_log,
        statusline_at,
        settings.calibration.as_ref().map(|value| value.recorded_at),
    ]
    .into_iter()
    .flatten()
    .max();
    let text = tray_menu::format_menu(
        claude,
        codex,
        claude_on,
        codex_on,
        needs_auth && settings.claude_official_query,
        updated_at,
        Utc::now(),
        settings.menu_bar_quota,
    );
    let menu = app.state::<TrayState>();
    let line_count = text.lines.len().saturating_sub(1).min(menu.lines.len());
    for (item, line) in menu.lines.iter().zip(
        text.lines
            .iter()
            .take(text.lines.len().saturating_sub(1))
            .map(String::as_str)
            .chain(std::iter::repeat("")),
    ) {
        let _ = item.set_text(line);
        let _ = item.set_enabled(false);
    }
    let _ = menu.authorization.set_enabled(text.needs_authorization);
    let _ = menu.authorization.set_text(if text.needs_authorization {
        "Claude 需要授权，点此打开应用"
    } else {
        ""
    });
    if let Some(updated) = text.lines.last() {
        let _ = menu.updated.set_text(updated);
    }
    let update = crate::updates::available(app);
    if let Some(update) = &update {
        let _ = menu
            .update
            .set_text(format!("新版本 {} 可以更新…", update.version));
    }
    let wanted = (line_count, text.needs_authorization, update.is_some());
    if let Ok(mut layout) = menu.layout.lock() {
        if *layout != wanted && rebuild_menu(app, &menu, wanted.0, wanted.1, wanted.2).is_ok() {
            *layout = wanted;
        }
    }
    let _ = menu.alerts.set_checked(settings.advice_notifications);
    let login = login_item::get(app);
    let _ = menu.autostart.set_checked(login.enabled);
    let _ = menu.autostart.set_enabled(login.available);
    let _ = menu.silent.set_checked(settings.silent_start);
    for (item, mode) in menu.modes.iter().zip([
        MenuBarQuota::Off,
        MenuBarQuota::Claude,
        MenuBarQuota::Codex,
        MenuBarQuota::Both,
    ]) {
        let _ = item.set_checked(settings.menu_bar_quota == mode);
    }
    #[cfg(target_os = "macos")]
    let _ = menu.tray.set_title(text.title.as_deref());
    let _ = menu.tray.set_tooltip(Some(text.tooltip.as_str()));
}

pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn hide_main(window: &tauri::Window, data_dir: &Path) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    let _ = window
        .app_handle()
        .set_activation_policy(tauri::ActivationPolicy::Accessory);
    let Ok(mut current) = settings::read_settings(data_dir) else {
        return;
    };
    if current.tray_hint_shown {
        return;
    }
    current.tray_hint_shown = true;
    if settings::write_settings(data_dir, &current).is_err() {
        return;
    }
    let place = if cfg!(target_os = "macos") {
        "菜单栏"
    } else {
        "任务栏右下角"
    };
    crate::notifications::post_later(
        window.app_handle(),
        Duration::from_millis(1500),
        "token-audit 仍在运行".to_owned(),
        format!("关闭窗口后会继续检测并发送省额度提醒。可以从{place}的图标重新打开或退出。"),
    );
}
