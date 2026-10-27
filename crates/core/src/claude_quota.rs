//! Opt-in, read-only Claude OAuth usage query. Secrets never appear in errors or results.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::{path::PathBuf, process::Stdio, sync::mpsc, time::Instant};

#[cfg(any(target_os = "macos", test))]
use aes::Aes128;
#[cfg(any(target_os = "macos", test))]
use base64::Engine;
#[cfg(any(target_os = "macos", test))]
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use chrono::{DateTime, TimeZone, Utc};
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroizing;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const EXPIRY_MARGIN_SECONDS: i64 = 60;
const MAX_CREDENTIAL_BYTES: u64 = 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    ClaudeDesktop,
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialPreference {
    #[default]
    Auto,
    ClaudeDesktop,
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretError {
    Denied,
    Unavailable,
}

pub trait SecretStore {
    fn generic_password(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError>;

    /// Like [`generic_password`](Self::generic_password) but never shows a system
    /// prompt: a secret that needs the user's approval reads as absent. Used for
    /// background queries, so only a user action can bring up a password dialog.
    fn generic_password_silently(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let _ = (service, account);
        Ok(None)
    }
}

type SecretKey = (String, Option<String>);

/// Remembers secrets read during this run, so repeated quota queries do not ask the
/// system keychain (and the user) again. Nothing is written to disk; [`forget`]
/// drops every remembered secret, e.g. after Anthropic rejects the token.
///
/// [`forget`]: MemorySecretStore::forget
#[derive(Clone)]
pub struct MemorySecretStore<S> {
    inner: S,
    remembered: Arc<Mutex<HashMap<SecretKey, Zeroizing<Vec<u8>>>>>,
    silent: bool,
}

impl<S: Clone> MemorySecretStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            remembered: Arc::default(),
            silent: false,
        }
    }

    /// The same store, sharing what it remembers, but reading the keychain only
    /// without prompts (see [`SecretStore::generic_password_silently`]).
    #[must_use]
    pub fn silent(&self) -> Self {
        Self {
            silent: true,
            ..self.clone()
        }
    }

    pub fn forget(&self) {
        self.remembered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

impl<S: SecretStore> SecretStore for MemorySecretStore<S> {
    fn generic_password(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let key = (service.to_owned(), account.map(str::to_owned));
        if let Some(secret) = self
            .remembered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Some(secret.clone()));
        }
        let secret = if self.silent {
            self.inner.generic_password_silently(service, account)?
        } else {
            self.inner.generic_password(service, account)?
        };
        if let Some(secret) = &secret {
            self.remembered
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, secret.clone());
        }
        Ok(secret)
    }
}

impl ClaudeQuotaError {
    /// Errors after which a remembered credential should be read afresh.
    #[must_use]
    pub fn stale_credential(&self) -> bool {
        matches!(
            self,
            Self::TokenExpired { .. } | Self::Unauthorized { .. } | Self::Parse
        )
    }
}

#[derive(Clone)]
#[cfg_attr(not(target_os = "macos"), derive(Default))]
pub struct SystemSecretStore {
    #[cfg(target_os = "macos")]
    security_path: PathBuf,
    #[cfg(target_os = "macos")]
    security_timeout: Duration,
}

#[cfg(target_os = "macos")]
impl Default for SystemSecretStore {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            security_path: PathBuf::from("/usr/bin/security"),
            #[cfg(target_os = "macos")]
            security_timeout: Duration::from_secs(10),
        }
    }
}

impl SystemSecretStore {
    #[cfg(target_os = "macos")]
    #[doc(hidden)]
    pub fn with_security_path(path: PathBuf, timeout: Duration) -> Self {
        Self {
            security_path: path,
            security_timeout: timeout,
        }
    }

    #[cfg(target_os = "macos")]
    fn code_password(
        &self,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let mut command = std::process::Command::new(&self.security_path);
        command.args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ]);
        if let Some(account) = account {
            command.args(["-a", account]);
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SecretError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(SecretError::Unavailable)?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            let result = stdout
                .take(MAX_CREDENTIAL_BYTES + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
                .map_err(|_| SecretError::Unavailable);
            let _ = sender.send(result);
        });
        let deadline = Instant::now() + self.security_timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SecretError::Unavailable);
                }
            }
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut bytes = receiver
            .recv_timeout(remaining)
            .map_err(|_| SecretError::Unavailable)??;
        if status.code() == Some(44) {
            return Ok(None);
        }
        if !status.success() || bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
            return Err(SecretError::Unavailable);
        }
        while bytes
            .last()
            .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
        {
            bytes.pop();
        }
        Ok(Some(bytes))
    }
}

impl SecretStore for SystemSecretStore {
    fn generic_password(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        #[cfg(target_os = "macos")]
        {
            if service == "Claude Code-credentials" {
                return self.code_password(account);
            }
            let account = match account {
                Some(account) => account.to_owned(),
                None => {
                    let output = std::process::Command::new("/usr/bin/id")
                        .arg("-un")
                        .output()
                        .map_err(|_| SecretError::Unavailable)?;
                    if !output.status.success() {
                        return Err(SecretError::Unavailable);
                    }
                    String::from_utf8(output.stdout)
                        .map_err(|_| SecretError::Unavailable)?
                        .trim()
                        .to_owned()
                }
            };
            match security_framework::passwords::get_generic_password(service, &account) {
                Ok(password) => Ok(Some(Zeroizing::new(password))),
                Err(error) if error.code() == -25300 => Ok(None),
                Err(error) if matches!(error.code(), -128 | -25293) => Err(SecretError::Denied),
                Err(_) => Err(SecretError::Unavailable),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (service, account);
            Ok(None)
        }
    }

    fn generic_password_silently(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        #[cfg(target_os = "macos")]
        {
            // The Claude Code item is read through `/usr/bin/security`, whose
            // prompt this process cannot suppress, so it is never read silently.
            if service == "Claude Code-credentials" {
                return Ok(None);
            }
            // While the lock lives, an item that needs approval fails with
            // errSecInteractionNotAllowed instead of opening a dialog.
            let _lock =
                security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()
                    .map_err(|_| SecretError::Unavailable)?;
            match self.generic_password(service, account) {
                Err(SecretError::Unavailable) => Ok(None),
                other => other,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.generic_password(service, account)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeQuotaError {
    NoCredential,
    CredentialDenied { source: CredentialSource },
    KeychainTimeout { source: CredentialSource },
    TokenExpired { source: CredentialSource },
    Unauthorized { error_type: Option<String> },
    RateLimited { retry_after: Option<Duration> },
    Http { status: u16 },
    Network,
    Parse,
    Unsupported,
}

impl ClaudeQuotaError {
    pub fn message(&self, lang: crate::i18n::Lang) -> String {
        use crate::tr;
        match self {
            Self::NoCredential => lang
                .pick(
                    "未找到 Claude 登录凭据",
                    "Claude login credentials not found",
                )
                .to_owned(),
            Self::CredentialDenied { source } => tr!(
                lang,
                "读取{}凭据被拒绝",
                "Access to {} credentials was denied",
                source.label()
            ),
            Self::KeychainTimeout { .. } => lang
                .pick(
                    "等待钥匙串授权超时",
                    "Timed out waiting for Keychain access",
                )
                .to_owned(),
            Self::TokenExpired { source } => tr!(
                lang,
                "{}的登录令牌已过期",
                "{} login token has expired",
                source.label()
            ),
            Self::Unauthorized { error_type } => {
                let base = lang.pick(
                    "Anthropic 拒绝了登录令牌",
                    "Anthropic rejected the login token",
                );
                error_type.as_ref().map_or_else(
                    || base.to_owned(),
                    |kind| tr!(lang, "{base}（{kind}）", "{base} ({kind})"),
                )
            }
            Self::RateLimited { .. } => lang
                .pick(
                    "Anthropic 查询已被限流",
                    "Anthropic quota query was rate limited",
                )
                .to_owned(),
            Self::Http { status } => tr!(
                lang,
                "Anthropic 查询失败（HTTP {status}）",
                "Anthropic quota query failed (HTTP {status})"
            ),
            Self::Network => lang
                .pick("无法连接 Anthropic", "Could not connect to Anthropic")
                .to_owned(),
            Self::Parse => lang
                .pick(
                    "无法解析 Claude 额度数据或凭据",
                    "Could not parse Claude quota data or credentials",
                )
                .to_owned(),
            Self::Unsupported => lang
                .pick(
                    "此平台暂不支持该凭据来源",
                    "This credential source is not supported on this platform",
                )
                .to_owned(),
        }
    }
}

impl std::fmt::Display for ClaudeQuotaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message(crate::i18n::Lang::Zh))
    }
}

impl std::error::Error for ClaudeQuotaError {}

impl CredentialSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude 桌面版",
            Self::ClaudeCode => "Claude Code",
        }
    }
}

pub struct QueryOptions {
    pub preference: CredentialPreference,
    pub proxy: Option<String>,
    pub timeout: Duration,
    pub keychain_timeout: Duration,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            preference: CredentialPreference::Auto,
            proxy: None,
            timeout: Duration::from_secs(15),
            keychain_timeout: Duration::from_secs(60),
        }
    }
}

pub struct UsageResponse {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
    pub retry_after: Option<String>,
}

pub trait UsageTransport {
    fn get_usage(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        proxy: Option<&str>,
        timeout: Duration,
    ) -> Result<UsageResponse, ClaudeQuotaError>;
}

pub struct UreqTransport;

impl UsageTransport for UreqTransport {
    fn get_usage(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        proxy: Option<&str>,
        timeout: Duration,
    ) -> Result<UsageResponse, ClaudeQuotaError> {
        if url != USAGE_URL {
            return Err(ClaudeQuotaError::Network);
        }
        let mut builder = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .timeout(timeout);
        if let Some(proxy) = proxy {
            if !proxy.starts_with("http://") {
                return Err(ClaudeQuotaError::Network);
            }
            builder =
                builder.proxy(ureq::Proxy::new(proxy).map_err(|_| ClaudeQuotaError::Network)?);
        }
        let agent = builder.build();
        let mut request = agent.get(USAGE_URL);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        let response = match request.call() {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(_) => return Err(ClaudeQuotaError::Network),
        };
        let status = response.status();
        let retry_after = response.header("retry-after").map(str::to_owned);
        let mut body = Zeroizing::new(Vec::new());
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| ClaudeQuotaError::Network)?;
        if body.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(ClaudeQuotaError::Parse);
        }
        Ok(UsageResponse {
            status,
            body,
            retry_after,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClaudeQuotaWindow {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownRow {
    pub key: String,
    pub display_name: Option<String>,
    pub percent: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeeklyBreakdown {
    pub as_of: Option<DateTime<Utc>>,
    pub window_started_at: Option<DateTime<Utc>>,
    pub rows: Vec<BreakdownRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtraUsage {
    #[serde(default, deserialize_with = "deserialize_bool")]
    pub is_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_number")]
    pub monthly_limit: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_number")]
    pub used_credits: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_number")]
    pub utilization: Option<f64>,
}

#[derive(Default)]
struct MaybeNumber(Option<f64>);

impl<'de> Deserialize<'de> for MaybeNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NumberVisitor;
        impl<'de> Visitor<'de> for NumberVisitor {
            type Value = MaybeNumber;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an optional number")
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(MaybeNumber(Some(value as f64)))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(MaybeNumber(Some(value as f64)))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(MaybeNumber(value.is_finite().then_some(value)))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_string<E: de::Error>(self, _: String) -> Result<Self::Value, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MaybeNumber(None))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(MaybeNumber(None))
            }
        }
        deserializer.deserialize_any(NumberVisitor)
    }
}

fn deserialize_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    MaybeNumber::deserialize(deserializer).map(|value| value.0)
}

fn deserialize_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    struct BoolVisitor;
    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = Option<bool>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an optional boolean")
        }
        fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
            Ok(Some(value))
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_string<E: de::Error>(self, _: String) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }
    deserializer.deserialize_any(BoolVisitor)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClaudeQuota {
    pub fetched_at: DateTime<Utc>,
    pub source: CredentialSource,
    pub fallback_reason: Option<String>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
    pub windows: Vec<ClaudeQuotaWindow>,
    pub weekly_breakdown: Option<WeeklyBreakdown>,
    pub extra_usage: Option<ExtraUsage>,
}

struct SecretString(Zeroizing<String>);

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|text| Self(Zeroizing::new(text)))
    }
}

#[derive(Deserialize)]
struct CodeFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<CodeOAuth>,
}

#[derive(Deserialize)]
struct CodeOAuth {
    #[serde(rename = "accessToken")]
    access_token: Option<SecretString>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
struct DesktopConfig {
    #[serde(rename = "oauth:tokenCacheV2")]
    cache_v2: Option<String>,
    #[serde(rename = "oauth:tokenCache")]
    cache: Option<String>,
}

struct Credential {
    token: Zeroizing<String>,
    expires_at: i64,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

fn expiration_seconds(timestamp: f64) -> Option<i64> {
    if !timestamp.is_finite() || timestamp < 0.0 {
        return None;
    }
    let seconds = if timestamp > 1e12 {
        timestamp / 1000.0
    } else {
        timestamp
    };
    if seconds > i64::MAX as f64 {
        None
    } else {
        Some(seconds as i64)
    }
}

fn read_limited(path: &Path) -> Result<Option<Zeroizing<Vec<u8>>>, ClaudeQuotaError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ClaudeQuotaError::NoCredential),
    };
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ClaudeQuotaError::NoCredential)?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(ClaudeQuotaError::Parse);
    }
    Ok(Some(bytes))
}

fn code_credential(store: &dyn SecretStore, home: &Path) -> Result<Credential, ClaudeQuotaError> {
    #[cfg(target_os = "macos")]
    let bytes = store
        .generic_password("Claude Code-credentials", None)
        .map_err(|error| match error {
            SecretError::Denied => ClaudeQuotaError::CredentialDenied {
                source: CredentialSource::ClaudeCode,
            },
            SecretError::Unavailable => ClaudeQuotaError::NoCredential,
        })?;
    #[cfg(not(target_os = "macos"))]
    let bytes = {
        let _ = store;
        read_limited(&home.join(".claude/.credentials.json"))?
    };
    #[cfg(target_os = "macos")]
    let _ = home;
    let bytes = bytes.ok_or(ClaudeQuotaError::NoCredential)?;
    #[cfg(target_os = "macos")]
    let parsed: CodeFile = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            let decoded = decode_security_hex(&bytes).ok_or(ClaudeQuotaError::Parse)?;
            serde_json::from_slice(&decoded).map_err(|_| ClaudeQuotaError::Parse)?
        }
    };
    #[cfg(not(target_os = "macos"))]
    let parsed: CodeFile = serde_json::from_slice(&bytes).map_err(|_| ClaudeQuotaError::Parse)?;
    let oauth = parsed
        .claude_ai_oauth
        .ok_or(ClaudeQuotaError::NoCredential)?;
    let token = oauth.access_token.ok_or(ClaudeQuotaError::NoCredential)?.0;
    let expires_at = oauth
        .expires_at
        .and_then(expiration_seconds)
        .ok_or(ClaudeQuotaError::Parse)?;
    Ok(Credential {
        token,
        expires_at,
        subscription_type: oauth.subscription_type,
        rate_limit_tier: oauth.rate_limit_tier,
    })
}

#[cfg(target_os = "macos")]
fn decode_security_hex(bytes: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let hex = bytes.strip_prefix(b"0x").unwrap_or(bytes);
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut decoded = Zeroizing::new(Vec::with_capacity(hex.len() / 2));
    for pair in hex.as_chunks::<2>().0 {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        decoded.push(((high << 4) | low) as u8);
    }
    Some(decoded)
}

// This visitor only materializes access tokens, expiry times, and plan metadata.
// Other fields, including refreshToken, are consumed as IgnoredAny.
#[cfg(any(target_os = "macos", test))]
struct CredentialNode(Option<Credential>);

#[cfg(any(target_os = "macos", test))]
impl<'de> Deserialize<'de> for CredentialNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;
        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = CredentialNode;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a credential cache value")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let (mut token, mut access_token, mut expiry, mut best) = (None, None, None, None);
                let (mut subscription_type, mut rate_limit_tier) = (None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "token" => token = map.next_value::<Option<SecretString>>()?.map(|v| v.0),
                        "accessToken" => {
                            access_token = map.next_value::<Option<SecretString>>()?.map(|v| v.0)
                        }
                        "expiresAt" => expiry = map.next_value::<Option<f64>>()?,
                        "subscriptionType" => {
                            subscription_type = map.next_value::<Option<String>>()?
                        }
                        "rateLimitTier" => rate_limit_tier = map.next_value::<Option<String>>()?,
                        "refreshToken" => {
                            let _: IgnoredAny = map.next_value()?;
                        }
                        _ => {
                            let child = map.next_value::<CredentialNode>()?.0;
                            if child.as_ref().is_some_and(|c| {
                                best.as_ref()
                                    .is_none_or(|b: &Credential| c.expires_at > b.expires_at)
                            }) {
                                best = child;
                            }
                        }
                    }
                }
                if let (Some(token), Some(expires_at)) =
                    (access_token.or(token), expiry.and_then(expiration_seconds))
                {
                    if best
                        .as_ref()
                        .is_none_or(|b: &Credential| expires_at > b.expires_at)
                    {
                        best = Some(Credential {
                            token,
                            expires_at,
                            subscription_type: subscription_type.clone(),
                            rate_limit_tier: rate_limit_tier.clone(),
                        });
                    }
                }
                if let Some(best) = best.as_mut() {
                    if best.subscription_type.is_none() {
                        best.subscription_type = subscription_type;
                    }
                    if best.rate_limit_tier.is_none() {
                        best.rate_limit_tier = rate_limit_tier;
                    }
                }
                Ok(CredentialNode(best))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut best: Option<Credential> = None;
                while let Some(child) = seq.next_element::<CredentialNode>()? {
                    if child
                        .0
                        .as_ref()
                        .is_some_and(|c| best.as_ref().is_none_or(|b| c.expires_at > b.expires_at))
                    {
                        best = child.0;
                    }
                }
                Ok(CredentialNode(best))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_string<E: de::Error>(self, _: String) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(CredentialNode(None))
            }
        }
        deserializer.deserialize_any(NodeVisitor)
    }
}

#[cfg(target_os = "macos")]
fn desktop_credential<S: SecretStore + Send + 'static>(
    store: S,
    home: &Path,
    keychain_timeout: Duration,
) -> Result<Credential, ClaudeQuotaError> {
    let bytes = read_limited(&home.join("Library/Application Support/Claude/config.json"))?
        .ok_or(ClaudeQuotaError::NoCredential)?;
    let config: DesktopConfig =
        serde_json::from_slice(&bytes).map_err(|_| ClaudeQuotaError::Parse)?;
    if config.cache_v2.is_none() && config.cache.is_none() {
        return Err(ClaudeQuotaError::NoCredential);
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = store.generic_password("Claude Safe Storage", Some("Claude"));
        let _ = sender.send(result);
    });
    // A timed-out Security.framework call may remain blocked on macOS's consent dialog.
    // The detached thread ends when the user responds or the process exits.
    let password = receiver
        .recv_timeout(keychain_timeout)
        .map_err(|_| ClaudeQuotaError::KeychainTimeout {
            source: CredentialSource::ClaudeDesktop,
        })?
        .map_err(|error| match error {
            SecretError::Denied => ClaudeQuotaError::CredentialDenied {
                source: CredentialSource::ClaudeDesktop,
            },
            SecretError::Unavailable => ClaudeQuotaError::NoCredential,
        })?
        .ok_or(ClaudeQuotaError::NoCredential)?;
    let mut first_error = None;
    let mut expired = None;
    for blob in [config.cache_v2, config.cache].into_iter().flatten() {
        match decrypt_desktop_cache(&password, &blob) {
            Ok(credential)
                if credential.expires_at > Utc::now().timestamp() + EXPIRY_MARGIN_SECONDS =>
            {
                return Ok(credential)
            }
            Ok(credential) if expired.is_none() => expired = Some(credential),
            Ok(_) => {}
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    expired.ok_or_else(|| first_error.unwrap_or(ClaudeQuotaError::NoCredential))
}

#[cfg(not(target_os = "macos"))]
fn desktop_credential<S: SecretStore>(
    _: S,
    _: &Path,
    _: Duration,
) -> Result<Credential, ClaudeQuotaError> {
    Err(ClaudeQuotaError::Unsupported)
}

#[cfg(any(target_os = "macos", test))]
fn decrypt_desktop_cache(password: &[u8], blob: &str) -> Result<Credential, ClaudeQuotaError> {
    let mut encrypted = Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(blob)
            .map_err(|_| ClaudeQuotaError::Parse)?,
    );
    if !encrypted.starts_with(b"v10") || encrypted.len() <= 3 || (encrypted.len() - 3) % 16 != 0 {
        return Err(ClaudeQuotaError::Parse);
    }
    let mut key = Zeroizing::new([0_u8; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1003, &mut *key);
    let decrypted = cbc::Decryptor::<Aes128>::new_from_slices(&*key, b"                ")
        .map_err(|_| ClaudeQuotaError::Parse)?
        .decrypt_padded_mut::<Pkcs7>(&mut encrypted[3..])
        .map_err(|_| ClaudeQuotaError::Parse)?;
    let node: CredentialNode =
        serde_json::from_slice(decrypted).map_err(|_| ClaudeQuotaError::Parse)?;
    node.0.ok_or(ClaudeQuotaError::NoCredential)
}

fn credential_for<S: SecretStore + Clone + Send + 'static>(
    source: CredentialSource,
    store: &S,
    home: &Path,
    keychain_timeout: Duration,
) -> Result<Credential, ClaudeQuotaError> {
    match source {
        CredentialSource::ClaudeDesktop => {
            desktop_credential(store.clone(), home, keychain_timeout)
        }
        CredentialSource::ClaudeCode => code_credential(store, home),
    }
}

#[derive(Deserialize)]
struct WindowFields {
    #[serde(default)]
    utilization: MaybeNumber,
    resets_at: Option<serde_json::Value>,
}

struct WindowMaybe(Option<WindowFields>);

impl<'de> Deserialize<'de> for WindowMaybe {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WindowVisitor;
        impl<'de> Visitor<'de> for WindowVisitor {
            type Value = WindowMaybe;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a usage window")
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                WindowFields::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|v| WindowMaybe(Some(v)))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_string<E: de::Error>(self, _: String) -> Result<Self::Value, E> {
                Ok(WindowMaybe(None))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(WindowMaybe(None))
            }
        }
        deserializer.deserialize_any(WindowVisitor)
    }
}

struct UsagePayload {
    windows: Vec<ClaudeQuotaWindow>,
    weekly_breakdown: Option<WeeklyBreakdown>,
    extra_usage: Option<ExtraUsage>,
}

#[derive(Deserialize)]
struct BreakdownFields {
    as_of: Option<serde_json::Value>,
    window_started_at: Option<serde_json::Value>,
    #[serde(default)]
    rows: Vec<BreakdownRowFields>,
}

#[derive(Deserialize)]
struct BreakdownRowFields {
    #[serde(default)]
    key: serde_json::Value,
    #[serde(default)]
    display_name: serde_json::Value,
    #[serde(default)]
    percent: MaybeNumber,
}

impl From<BreakdownFields> for WeeklyBreakdown {
    fn from(fields: BreakdownFields) -> Self {
        Self {
            as_of: fields.as_of.as_ref().and_then(parse_reset),
            window_started_at: fields.window_started_at.as_ref().and_then(parse_reset),
            rows: fields
                .rows
                .into_iter()
                .filter_map(|row| {
                    Some(BreakdownRow {
                        key: row.key.as_str()?.to_owned(),
                        display_name: row.display_name.as_str().map(str::to_owned),
                        percent: row.percent.0?,
                    })
                })
                .collect(),
        }
    }
}

fn is_usage_window(name: &str) -> bool {
    matches!(
        name,
        "five_hour"
            | "seven_day"
            | "seven_day_opus"
            | "seven_day_sonnet"
            | "seven_day_cowork"
            | "seven_day_oauth_apps"
    )
}

impl<'de> Deserialize<'de> for UsagePayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PayloadVisitor;
        impl<'de> Visitor<'de> for PayloadVisitor {
            type Value = UsagePayload;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a usage object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut windows = Vec::new();
                let mut weekly_breakdown = None;
                let mut extra_usage = None;
                while let Some(name) = map.next_key::<String>()? {
                    if name == "extra_usage" {
                        extra_usage = map.next_value()?;
                    } else if name == "seven_day_breakdown" {
                        weekly_breakdown = map
                            .next_value::<Option<BreakdownFields>>()?
                            .map(WeeklyBreakdown::from);
                    } else if is_usage_window(&name) {
                        if let Some(fields) = map.next_value::<WindowMaybe>()?.0 {
                            if let Some(utilization) = fields.utilization.0 {
                                windows.push(ClaudeQuotaWindow {
                                    name,
                                    utilization,
                                    resets_at: fields.resets_at.as_ref().and_then(parse_reset),
                                });
                            }
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                windows.sort_by(|a, b| a.name.cmp(&b.name));
                Ok(UsagePayload {
                    windows,
                    weekly_breakdown,
                    extra_usage,
                })
            }
        }
        deserializer.deserialize_map(PayloadVisitor)
    }
}

fn parse_reset(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|time| time.with_timezone(&Utc));
    }
    let timestamp = value.as_f64().and_then(expiration_seconds)?;
    Utc.timestamp_opt(timestamp, 0).single()
}

fn fallback_reason(error: &ClaudeQuotaError) -> Option<&'static str> {
    match error {
        ClaudeQuotaError::NoCredential => Some("未找到 Claude 桌面版凭据"),
        ClaudeQuotaError::TokenExpired { .. } => Some("Claude 桌面版令牌已过期"),
        ClaudeQuotaError::CredentialDenied { .. } => Some("Claude 桌面版钥匙串访问被拒绝"),
        ClaudeQuotaError::KeychainTimeout { .. } => Some("等待 Claude 桌面版钥匙串授权超时"),
        ClaudeQuotaError::Unsupported => Some("此平台暂不支持 Claude 桌面版凭据"),
        _ => None,
    }
}

pub fn query_claude_quota<S: SecretStore + Clone + Send + Sync + 'static>(
    store: &S,
    home: &Path,
    transport: &dyn UsageTransport,
    options: &QueryOptions,
) -> Result<ClaudeQuota, ClaudeQuotaError> {
    let mut fallback = None;
    let (source, credential) = if options.preference == CredentialPreference::Auto {
        match credential_for(
            CredentialSource::ClaudeDesktop,
            store,
            home,
            options.keychain_timeout,
        ) {
            Ok(credential)
                if credential.expires_at > Utc::now().timestamp() + EXPIRY_MARGIN_SECONDS =>
            {
                (CredentialSource::ClaudeDesktop, credential)
            }
            Ok(_) => {
                fallback = Some("Claude 桌面版令牌已过期".to_owned());
                (
                    CredentialSource::ClaudeCode,
                    credential_for(
                        CredentialSource::ClaudeCode,
                        store,
                        home,
                        options.keychain_timeout,
                    )?,
                )
            }
            Err(error) if fallback_reason(&error).is_some() => {
                fallback = fallback_reason(&error).map(str::to_owned);
                (
                    CredentialSource::ClaudeCode,
                    credential_for(
                        CredentialSource::ClaudeCode,
                        store,
                        home,
                        options.keychain_timeout,
                    )?,
                )
            }
            Err(error) => return Err(error),
        }
    } else {
        let source = match options.preference {
            CredentialPreference::ClaudeDesktop => CredentialSource::ClaudeDesktop,
            _ => CredentialSource::ClaudeCode,
        };
        (
            source,
            credential_for(source, store, home, options.keychain_timeout)?,
        )
    };
    if credential.expires_at <= Utc::now().timestamp() + EXPIRY_MARGIN_SECONDS {
        return Err(ClaudeQuotaError::TokenExpired { source });
    }
    let mut authorization = Zeroizing::new(format!("Bearer {}", credential.token.as_str()));
    let headers = [
        ("Authorization", authorization.as_str()),
        ("anthropic-beta", "oauth-2025-04-20"),
        (
            "User-Agent",
            concat!("token-audit/", env!("CARGO_PKG_VERSION")),
        ),
        ("Accept", "application/json"),
    ];
    let response = transport.get_usage(
        USAGE_URL,
        &headers,
        options.proxy.as_deref(),
        options.timeout,
    )?;
    zeroize::Zeroize::zeroize(&mut *authorization);
    match response.status {
        200 => {
            let payload: UsagePayload =
                serde_json::from_slice(&response.body).map_err(|_| ClaudeQuotaError::Parse)?;
            Ok(ClaudeQuota {
                fetched_at: Utc::now(),
                source,
                fallback_reason: fallback,
                subscription_type: credential.subscription_type,
                rate_limit_tier: credential.rate_limit_tier,
                windows: payload.windows,
                weekly_breakdown: payload.weekly_breakdown,
                extra_usage: payload.extra_usage,
            })
        }
        401 | 403 => {
            #[derive(Deserialize)]
            struct ErrorBody {
                error: Option<ErrorDetails>,
            }
            #[derive(Deserialize)]
            struct ErrorDetails {
                #[serde(rename = "type")]
                error_type: Option<String>,
            }
            let error_type = serde_json::from_slice::<ErrorBody>(&response.body)
                .ok()
                .and_then(|body| body.error.and_then(|error| error.error_type))
                .filter(|value| {
                    value.len() <= 64 && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                });
            Err(ClaudeQuotaError::Unauthorized { error_type })
        }
        429 => Err(ClaudeQuotaError::RateLimited {
            retry_after: response
                .retry_after
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs),
        }),
        status => Err(ClaudeQuotaError::Http { status }),
    }
}

pub fn parse_scutil_proxy(output: &str) -> Option<String> {
    let fields: std::collections::HashMap<_, _> = output
        .lines()
        .filter_map(|line| line.trim().split_once(" : "))
        .collect();
    if fields.get("ProxyAutoConfigEnable") == Some(&"1")
        || fields.get("ProxyAutoDiscoveryEnable") == Some(&"1")
    {
        return None;
    }
    for (enabled, host, port) in [
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
        ("HTTPEnable", "HTTPProxy", "HTTPPort"),
    ] {
        if fields.get(enabled) == Some(&"1") {
            if let (Some(host), Some(port)) = (fields.get(host), fields.get(port)) {
                if !host.is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0) {
                    return Some(format!("http://{host}:{port}"));
                }
            }
        }
    }
    None
}

pub fn parse_windows_proxy(enabled: bool, server: &str) -> Option<String> {
    if !enabled {
        return None;
    }
    let entry = if server.contains('=') {
        server
            .split(';')
            .find_map(|part| part.trim().strip_prefix("https="))
            .or_else(|| {
                server
                    .split(';')
                    .find_map(|part| part.trim().strip_prefix("http="))
            })?
    } else {
        server.trim()
    };
    let (host, port) = entry.rsplit_once(':')?;
    if host.is_empty() || !port.parse::<u16>().is_ok_and(|port| port > 0) {
        return None;
    }
    Some(format!("http://{entry}"))
}

pub fn system_proxy() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/sbin/scutil")
            .arg("--proxy")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_scutil_proxy(std::str::from_utf8(&output.stdout).ok()?)
    }
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .ok()?;
        let enabled: u32 = key.get_value("ProxyEnable").ok()?;
        let server: String = key.get_value("ProxyServer").ok()?;
        parse_windows_proxy(enabled != 0, &server)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{decrypt_desktop_cache, ClaudeQuotaError};
    use crate::i18n::Lang;
    use aes::Aes128;
    use base64::Engine;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    const PASSWORD: &[u8] = b"fake-safe-storage-password";
    const BLOB: &str = include_str!("../tests/fixtures/claude_quota/desktop_blob.txt");

    #[test]
    fn quota_error_message_follows_language() {
        assert_eq!(
            ClaudeQuotaError::Network.message(Lang::Zh),
            "无法连接 Anthropic"
        );
        assert_eq!(
            ClaudeQuotaError::Network.message(Lang::En),
            "Could not connect to Anthropic"
        );
        assert_eq!(ClaudeQuotaError::Network.to_string(), "无法连接 Anthropic");
    }

    #[test]
    fn desktop_vector_selects_latest_valid_token() {
        let credential = decrypt_desktop_cache(PASSWORD, BLOB.trim()).expect("decrypt fake cache");
        assert!(credential.token.as_str() == "fake-access-token-AAA");
        assert_eq!(credential.expires_at, 4_102_444_800);
    }

    #[test]
    fn desktop_rejects_bad_encoding_prefix_padding_and_json() {
        assert!(matches!(
            decrypt_desktop_cache(PASSWORD, "!!!"),
            Err(ClaudeQuotaError::Parse)
        ));
        let mut wrong_prefix = base64::engine::general_purpose::STANDARD
            .decode(BLOB.trim())
            .expect("fixture base64");
        wrong_prefix[0] = b'x';
        let encoded = base64::engine::general_purpose::STANDARD.encode(&wrong_prefix);
        assert!(matches!(
            decrypt_desktop_cache(PASSWORD, &encoded),
            Err(ClaudeQuotaError::Parse)
        ));
        let mut bad_padding = base64::engine::general_purpose::STANDARD
            .decode(BLOB.trim())
            .expect("fixture base64");
        let last = bad_padding.len() - 1;
        bad_padding[last] ^= 0xff;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bad_padding);
        assert!(matches!(
            decrypt_desktop_cache(PASSWORD, &encoded),
            Err(ClaudeQuotaError::Parse)
        ));

        let mut key = [0_u8; 16];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(PASSWORD, b"saltysalt", 1003, &mut key);
        assert_eq!(
            key.map(|byte| format!("{byte:02x}")).concat(),
            "d6c4cbc56a7e53b00e27f34a5127e435"
        );
        let mut buffer = [0_u8; 16];
        buffer[..8].copy_from_slice(b"not json");
        let encrypted = cbc::Encryptor::<Aes128>::new_from_slices(&key, b"                ")
            .expect("test key and iv")
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, 8)
            .expect("test encryption");
        let blob = base64::engine::general_purpose::STANDARD
            .encode([b"v10".as_slice(), encrypted].concat());
        assert!(matches!(
            decrypt_desktop_cache(PASSWORD, &blob),
            Err(ClaudeQuotaError::Parse)
        ));
    }
}
