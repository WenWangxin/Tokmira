use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::claude_quota::CredentialPreference;
use crate::model::Tool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuBarQuota {
    Off,
    Claude,
    Codex,
    #[default]
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub tool: Tool,
    pub five_hour_percent: f64,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub onboarding_completed: bool,
    pub claude_official_query: bool,
    pub claude_credential: CredentialPreference,
    pub codex_app_server: bool,
    pub theme: Theme,
    pub calibration: Option<Calibration>,
    pub dismissed_advice: Vec<String>,
    /// System notification when a new advice problem shows up while the app runs.
    pub advice_notifications: bool,
    /// Set after the first "still running in the menu bar" notice.
    pub tray_hint_shown: bool,
    pub silent_start: bool,
    pub menu_bar_quota: MenuBarQuota,
    /// Off by default: checking for updates is one of the two allowed network requests.
    pub auto_check_updates: bool,
    pub update_last_checked_at: Option<DateTime<Utc>>,
    /// The newest version already announced, so each release is announced once.
    pub update_notified_version: Option<String>,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            onboarding_completed: false,
            claude_official_query: false,
            claude_credential: CredentialPreference::Auto,
            codex_app_server: false,
            theme: Theme::System,
            calibration: None,
            dismissed_advice: Vec::new(),
            advice_notifications: true,
            tray_hint_shown: false,
            silent_start: false,
            menu_bar_quota: MenuBarQuota::Both,
            auto_check_updates: false,
            update_last_checked_at: None,
            update_notified_version: None,
        }
    }
}

pub fn read_settings(data_dir: &Path) -> io::Result<AppSettings> {
    match fs::read(data_dir.join("settings.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(error) => Err(error),
    }
}
pub fn write_settings(data_dir: &Path, settings: &AppSettings) -> io::Result<()> {
    if settings.calibration.as_ref().is_some_and(|c| {
        !c.five_hour_percent.is_finite() || !(0.0..=100.0).contains(&c.five_hour_percent)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "校准百分比应在 0 到 100 之间",
        ));
    }
    fs::create_dir_all(data_dir)?;
    let temp = data_dir.join(format!(".settings-{}.tmp", std::process::id()));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).map_err(io::Error::other)?,
    )?;
    fs::rename(temp, data_dir.join("settings.json"))
}
