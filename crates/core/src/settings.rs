use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::claude_quota::CredentialPreference;
use crate::i18n::Lang;
use crate::model::Tool;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Interface language; `System` follows the OS locale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    Zh,
    En,
}
impl Language {
    /// `system_locale` is asked only when the user follows the system.
    pub fn resolve(self, system_locale: impl FnOnce() -> Option<String>) -> Lang {
        match self {
            Self::Zh => Lang::Zh,
            Self::En => Lang::En,
            Self::System => system_locale().map_or(Lang::Zh, |tag| Lang::from_locale(&tag)),
        }
    }
}

/// At most this many tools show their quota next to the menu bar icon.
pub const MENU_BAR_TOOLS_MAX: usize = 2;

/// Keeps the menu bar selection in the order picked, without repeats, and at most
/// [`MENU_BAR_TOOLS_MAX`] long.
pub fn normalize_menu_bar_tools(tools: &mut Vec<Tool>) {
    let mut seen = Vec::with_capacity(MENU_BAR_TOOLS_MAX);
    tools.retain(|tool| {
        let keep = !seen.contains(tool) && seen.len() < MENU_BAR_TOOLS_MAX;
        if keep {
            seen.push(*tool);
        }
        keep
    });
}

/// Adds `tool` to the menu bar selection, or removes it when already shown. Adding a
/// third tool replaces the one picked first.
pub fn toggle_menu_bar_tool(tools: &mut Vec<Tool>, tool: Tool) {
    if let Some(index) = tools.iter().position(|value| *value == tool) {
        tools.remove(index);
        return;
    }
    if tools.len() >= MENU_BAR_TOOLS_MAX {
        tools.remove(0);
    }
    tools.push(tool);
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
    pub language: Language,
    pub calibration: Option<Calibration>,
    pub dismissed_advice: Vec<String>,
    /// System notification when a new advice problem shows up while the app runs.
    pub advice_notifications: bool,
    /// Set after the first "still running in the menu bar" notice.
    pub tray_hint_shown: bool,
    pub silent_start: bool,
    /// Tools whose 5-hour quota shows next to the menu bar icon (macOS); empty shows
    /// the icon only. Replaces the old `menu_bar_quota`, which is ignored on read.
    pub menu_bar_tools: Vec<Tool>,
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
            language: Language::System,
            calibration: None,
            dismissed_advice: Vec::new(),
            advice_notifications: true,
            tray_hint_shown: false,
            silent_start: false,
            menu_bar_tools: Vec::new(),
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
    let mut settings = settings.clone();
    normalize_menu_bar_tools(&mut settings.menu_bar_tools);
    let settings = &settings;
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
