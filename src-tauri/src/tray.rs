//! The persistent menu bar / notification area icon and its live quota menu.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager};
use token_audit_core::model::Tool;
use token_audit_core::settings;

use crate::{login_item, tray_menu, AppState};

const TOOLS: [Tool; 2] = [Tool::Claude, Tool::Codex];
/// Enough rows for every tool's 5-hour, weekly and forecast lines.
const ROWS: usize = 6;

struct TrayState {
    sender: Sender<()>,
    tray: TrayIcon,
    /// A heading with the tool's logo, then that tool's quota rows indented under it.
    headers: Vec<IconMenuItem<tauri::Wry>>,
    rows: Vec<IconMenuItem<tauri::Wry>>,
    authorization: IconMenuItem<tauri::Wry>,
    updated: MenuItem<tauri::Wry>,
    refresh: MenuItem<tauri::Wry>,
    dashboard: MenuItem<tauri::Wry>,
    advice: MenuItem<tauri::Wry>,
    alerts: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
    silent: CheckMenuItem<tauri::Wry>,
    startup_submenu: Submenu<tauri::Wry>,
    #[cfg(target_os = "macos")]
    icon_only: CheckMenuItem<tauri::Wry>,
    #[cfg(target_os = "macos")]
    shown_tools: Vec<CheckMenuItem<tauri::Wry>>,
    #[cfg(target_os = "macos")]
    mode_submenu: Submenu<tauri::Wry>,
    update: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    layout: Mutex<Option<Layout>>,
    /// The menu bar badges last drawn, so the minute tick does not redraw the same image.
    #[cfg(target_os = "macos")]
    badges: Mutex<Option<Vec<(Tool, String)>>>,
}

/// What decides the menu's structure; text changes alone keep the built menu.
#[derive(Clone, PartialEq)]
struct Layout {
    rows: Vec<usize>,
    authorization: bool,
    update: bool,
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
                        claude_logs: false,
                    },
                );
            });
        }
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
        "menubar-off" => edit_settings(app, |s| s.menu_bar_tools.clear()),
        "menubar-Claude" => edit_settings(app, |s| {
            settings::toggle_menu_bar_tool(&mut s.menu_bar_tools, Tool::Claude);
        }),
        "menubar-Codex" => edit_settings(app, |s| {
            settings::toggle_menu_bar_tool(&mut s.menu_bar_tools, Tool::Codex);
        }),
        "quit" => app.exit(0),
        // Tool headings and quota rows open the dashboard, like "打开仪表盘".
        id if id == "dashboard"
            || id == "authorization"
            || id.starts_with("tool-")
            || id.starts_with("row-") =>
        {
            navigate(app, "/");
        }
        _ => {}
    }
}

fn menu_logo(tool: Tool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(match tool {
        Tool::Claude => include_bytes!("../icons/tray/claude-menu.png"),
        Tool::Codex => include_bytes!("../icons/tray/codex-menu.png"),
    })
}

/// A blank icon the size of the logos, so rows line up under the tool name.
fn indent() -> Image<'static> {
    Image::new_owned(vec![0; 36 * 36 * 4], 36, 36)
}

#[cfg(target_os = "macos")]
fn gauge() -> tauri::Result<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/tray-template.png"))
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let headers = TOOLS
        .into_iter()
        .map(|tool| {
            let name = tray_menu::tool_name(tool);
            IconMenuItem::with_id(
                app,
                format!("tool-{name}"),
                name,
                true,
                Some(menu_logo(tool)?),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let rows = (0..ROWS)
        .map(|i| {
            IconMenuItem::with_id(
                app,
                format!("row-{i}"),
                "",
                true,
                Some(indent()),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let authorization = IconMenuItem::with_id(
        app,
        "authorization",
        "需要授权，点此打开应用",
        true,
        Some(indent()),
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
    let startup_submenu = Submenu::with_id_and_items(
        app,
        "startup-menu",
        "启动方式",
        true,
        &[&autostart, &silent],
    )?;
    #[cfg(target_os = "macos")]
    let icon_only =
        CheckMenuItem::with_id(app, "menubar-off", "只显示图标", true, false, None::<&str>)?;
    #[cfg(target_os = "macos")]
    let shown_tools = TOOLS
        .into_iter()
        .map(|tool| {
            let name = tray_menu::tool_name(tool);
            CheckMenuItem::with_id(
                app,
                format!("menubar-{name}"),
                name,
                true,
                false,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    #[cfg(target_os = "macos")]
    let mode_submenu = {
        let submenu = Submenu::with_id(app, "menubar-menu", "菜单栏显示", true)?;
        submenu.append(&icon_only)?;
        submenu.append(&PredefinedMenuItem::separator(app)?)?;
        for item in &shown_tools {
            submenu.append(item)?;
        }
        submenu
    };
    let update_item = MenuItem::with_id(app, "update", "", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 token-audit", true, None::<&str>)?;
    let tray = TrayIconBuilder::with_id("main")
        .tooltip("token-audit")
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| menu_event(app, event.id().as_ref()));
    #[cfg(target_os = "macos")]
    let tray = tray.icon(gauge()?).icon_as_template(true);
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
        headers,
        rows,
        authorization,
        updated,
        refresh,
        dashboard,
        advice,
        alerts,
        autostart,
        silent,
        startup_submenu,
        #[cfg(target_os = "macos")]
        icon_only,
        #[cfg(target_os = "macos")]
        shown_tools,
        #[cfg(target_os = "macos")]
        mode_submenu,
        update: update_item,
        settings,
        quit,
        layout: Mutex::new(None),
        #[cfg(target_os = "macos")]
        badges: Mutex::new(None),
    });
    let app = app.handle().clone();
    // A menu exists from the start; the first update fills in the numbers.
    let state = app.state::<TrayState>();
    let first = Layout {
        rows: vec![1; TOOLS.len()],
        authorization: false,
        update: false,
    };
    if rebuild_menu(&app, &state, &first).is_ok() {
        if let Ok(mut layout) = state.layout.lock() {
            *layout = Some(first);
        }
    }
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

fn rebuild_menu(app: &AppHandle, state: &TrayState, layout: &Layout) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    let mut rows = state.rows.iter();
    for ((header, count), tool) in state.headers.iter().zip(&layout.rows).zip(TOOLS) {
        menu.append(header)?;
        for row in rows.by_ref().take(*count) {
            menu.append(row)?;
        }
        // The Claude authorization prompt sits with Claude's own rows.
        if tool == Tool::Claude && layout.authorization {
            menu.append(&state.authorization)?;
        }
    }
    menu.append(&state.updated)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for item in [&state.refresh, &state.dashboard, &state.advice] {
        menu.append(item)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&state.alerts)?;
    #[cfg(target_os = "macos")]
    menu.append(&state.mode_submenu)?;
    menu.append(&state.startup_submenu)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if layout.update {
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
        &settings.menu_bar_tools,
    );
    let menu = app.state::<TrayState>();
    let mut rows = menu.rows.iter();
    let mut counts = Vec::new();
    for (header, section) in menu.headers.iter().zip(&text.sections) {
        let _ = header.set_text(&section.heading);
        let mut count = 0;
        // The section's rows lead the zip, so the shared pool loses no item at its end.
        for (line, item) in section.rows.iter().zip(rows.by_ref()) {
            let _ = item.set_text(line);
            count += 1;
        }
        counts.push(count);
    }
    let _ = menu.updated.set_text(&text.updated);
    let update = crate::updates::available(app);
    if let Some(update) = &update {
        let _ = menu
            .update
            .set_text(format!("新版本 {} 可以更新…", update.version));
    }
    let wanted = Layout {
        rows: counts,
        authorization: text.needs_authorization,
        update: update.is_some(),
    };
    if let Ok(mut layout) = menu.layout.lock() {
        if layout.as_ref() != Some(&wanted) && rebuild_menu(app, &menu, &wanted).is_ok() {
            *layout = Some(wanted);
        }
    }
    let _ = menu.alerts.set_checked(settings.advice_notifications);
    let login = login_item::get(app);
    let _ = menu.autostart.set_checked(login.enabled);
    let _ = menu.autostart.set_enabled(login.available);
    let _ = menu.silent.set_checked(settings.silent_start);
    #[cfg(target_os = "macos")]
    {
        let _ = menu
            .icon_only
            .set_checked(settings.menu_bar_tools.is_empty());
        for (item, tool) in menu.shown_tools.iter().zip(TOOLS) {
            let _ = item.set_checked(settings.menu_bar_tools.contains(&tool));
        }
        if let Ok(mut drawn) = menu.badges.lock() {
            if drawn.as_ref() != Some(&text.badges) {
                // Offscreen bitmap drawing is safe off the main thread; the icon setter
                // hands the result to it.
                let icon = match crate::tray_image::render(&text.badges) {
                    Some((rgba, width, height)) => Ok(Image::new_owned(rgba, width, height)),
                    None => gauge(),
                };
                if let Ok(icon) = icon {
                    if menu
                        .tray
                        .set_icon_with_as_template(Some(icon), true)
                        .is_ok()
                    {
                        *drawn = Some(text.badges.clone());
                    }
                }
            }
        }
    }
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
