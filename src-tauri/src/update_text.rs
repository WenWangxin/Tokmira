//! What the settings page shows about an update, and the plugin's errors in Chinese.
//! Depends only on the updater plugin, so the integration tests include this file directly.

use serde::Serialize;
use tauri_plugin_updater::{Error, Update};

#[derive(Clone, Debug, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

pub fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        notes: update
            .body
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(str::to_owned),
        date: update.date.map(|date| date.to_string()),
    }
}

pub fn message(error: &Error) -> String {
    match error {
        Error::Reqwest(_) | Error::Network(_) => "无法连接 GitHub，请检查网络后重试".to_owned(),
        Error::ReleaseNotFound
        | Error::Serialization(_)
        | Error::Semver(_)
        | Error::TargetNotFound(_)
        | Error::TargetsNotFound(_) => "更新清单格式不正确或缺少本机平台的安装包".to_owned(),
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. } => "安装包签名校验失败，已停止安装".to_owned(),
        Error::UnsupportedOs | Error::UnsupportedArch => "当前系统不支持自动更新".to_owned(),
        other => format!("更新失败：{other}"),
    }
}
