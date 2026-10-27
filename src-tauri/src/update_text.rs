//! What the settings page shows about an update, and the plugin's errors in Chinese.
//! Depends only on the updater plugin, so the integration tests include this file directly.

use serde::Serialize;
use tauri_plugin_updater::{Error, Update};
use token_audit_core::i18n::Lang;

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

pub fn message(error: &Error, lang: Lang) -> String {
    match error {
        Error::Reqwest(_) | Error::Network(_) => lang
            .pick(
                "无法连接 GitHub，请检查网络后重试",
                "Could not connect to GitHub. Check your connection and retry.",
            )
            .to_owned(),
        Error::ReleaseNotFound
        | Error::Serialization(_)
        | Error::Semver(_)
        | Error::TargetNotFound(_)
        | Error::TargetsNotFound(_) => lang
            .pick(
                "更新清单格式不正确或缺少本机平台的安装包",
                "Update manifest is invalid or lacks a package for this platform",
            )
            .to_owned(),
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. } => lang
            .pick(
                "安装包签名校验失败，已停止安装",
                "Package signature verification failed; installation stopped",
            )
            .to_owned(),
        Error::UnsupportedOs | Error::UnsupportedArch => lang
            .pick(
                "当前系统不支持自动更新",
                "Automatic updates are not supported on this system",
            )
            .to_owned(),
        other => token_audit_core::tr!(lang, "更新失败：{other}", "Update failed: {other}"),
    }
}
