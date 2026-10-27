//! Reads and changes the operating system's login item, without persisting a second state.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Copy, Serialize)]
pub struct AutostartState {
    pub enabled: bool,
    pub available: bool,
    pub requires_approval: bool,
}

#[cfg(target_os = "macos")]
fn native_available() -> bool {
    use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};
    let version = NSOperatingSystemVersion {
        majorVersion: 13,
        minorVersion: 0,
        patchVersion: 0,
    };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(version)
}

#[cfg(target_os = "macos")]
fn bundled() -> bool {
    use objc2_foundation::NSBundle;
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

pub fn get(app: &AppHandle) -> AutostartState {
    #[cfg(target_os = "macos")]
    {
        if !bundled() || tauri::is_dev() {
            return AutostartState {
                enabled: false,
                available: false,
                requires_approval: false,
            };
        }
        if native_available() {
            use objc2_service_management::{SMAppService, SMAppServiceStatus};
            let status = unsafe { SMAppService::mainAppService().status() };
            return AutostartState {
                enabled: status == SMAppServiceStatus::Enabled
                    || status == SMAppServiceStatus::RequiresApproval,
                available: true,
                requires_approval: status == SMAppServiceStatus::RequiresApproval,
            };
        }
    }
    AutostartState {
        enabled: app.autolaunch().is_enabled().unwrap_or(false),
        available: !tauri::is_dev(),
        requires_approval: false,
    }
}

pub fn set(app: &AppHandle, enabled: bool) -> Result<AutostartState, String> {
    if !get(app).available {
        return Err("开发版不能设置开机自动启动，请在打包后的应用里设置".to_owned());
    }
    #[cfg(target_os = "macos")]
    if native_available() {
        use objc2_service_management::SMAppService;
        let service = unsafe { SMAppService::mainAppService() };
        let changed = unsafe {
            if enabled {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
        };
        changed.map_err(|_| "设置开机自动启动失败".to_owned())?;
        return Ok(get(app));
    }
    let launcher = app.autolaunch();
    let changed = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    changed.map_err(|_| "设置开机自动启动失败".to_owned())?;
    Ok(get(app))
}

pub fn migrate(app: &AppHandle, home: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        use objc2_service_management::{SMAppService, SMAppServiceStatus};
        let old = home.join("Library/LaunchAgents/token-audit.plist");
        if tauri::is_dev() || !bundled() || !native_available() || !old.is_file() {
            return;
        }
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        if status == SMAppServiceStatus::Enabled
            || status == SMAppServiceStatus::RequiresApproval
            || unsafe { service.registerAndReturnError() }.is_ok()
        {
            // The old entry is removed only after the new one has been registered.
            let _ = app.autolaunch().disable();
            let _ = std::fs::remove_file(old);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, home);
}

pub fn open_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_service_management::SMAppService;
        if native_available() {
            unsafe { SMAppService::openSystemSettingsLoginItems() };
            return Ok(());
        }
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
            .spawn()
            .map(|_| ())
            .map_err(|_| "无法打开登录项设置".to_owned())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("ms-settings:startupapps")
            .spawn()
            .map(|_| ())
            .map_err(|_| "无法打开登录项设置".to_owned())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Err("当前系统不支持登录项设置".to_owned())
}
