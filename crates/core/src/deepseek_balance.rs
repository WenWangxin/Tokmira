//! DeepSeek wallet balance, queried only after the user turns it on.
//!
//! Requests go to DeepSeek alone: the account route to `platform.deepseek.com`, the
//! API key route to `api.deepseek.com/user/balance`. Tokens and keys are read from
//! Harness's own files on each query, used in memory and dropped; Harness's
//! `.credentials.yaml` is opened read-only and never written or cleared, even on 401.
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::deepseek_view::{BalanceOrigin, DeepSeekBalance};
use crate::i18n::Lang;

pub const ACCOUNT_ORIGIN: &str = "https://platform.deepseek.com";
pub const ACCOUNT_URL: &str = "https://platform.deepseek.com/api/v0/users/get_user_summary";
pub const API_KEY_URL: &str = "https://api.deepseek.com/user/balance";
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;
/// Platform's code for an expired or revoked account token.
const ACCOUNT_UNAUTHORIZED: i64 = 40003;

/// Which Harness login the balance query uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeepSeekCredential {
    /// The account login when there is one, otherwise the API key.
    #[default]
    Auto,
    Account,
    ApiKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekRoute {
    Account,
    ApiKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BalanceError {
    NoCredential,
    /// The login was rejected. Harness's credentials are left as they are.
    Unauthorized(DeepSeekRoute),
    Network,
    Parse,
    /// The wallet holds no yuan balance, only a currency this view does not show.
    Currency,
}

impl BalanceError {
    #[must_use]
    pub fn message(self, lang: Lang) -> String {
        match self {
            Self::NoCredential => lang.pick(
                "没有找到 DeepSeek Harness 的登录：请先在 Harness 里登录账号，或配置 DEEPSEEK_API_KEY",
                "No DeepSeek Harness login found: sign in to Harness first, or set DEEPSEEK_API_KEY",
            ),
            Self::Unauthorized(DeepSeekRoute::Account) => lang.pick(
                "DeepSeek 账号登录已失效，请在 Harness 里重新登录",
                "The DeepSeek account login has expired; sign in again in Harness",
            ),
            Self::Unauthorized(DeepSeekRoute::ApiKey) => lang.pick(
                "DeepSeek API Key 无效或已停用",
                "The DeepSeek API key is invalid or disabled",
            ),
            Self::Network => lang.pick(
                "无法连接 DeepSeek，余额保持上次的结果",
                "Could not reach DeepSeek; the last balance is kept",
            ),
            Self::Parse => lang.pick(
                "DeepSeek 返回的余额格式无法识别，接口可能已变更",
                "DeepSeek returned a balance this app does not recognize; the API may have changed",
            ),
            Self::Currency => lang.pick(
                "DeepSeek 账户没有人民币余额，暂不支持其他币种",
                "The DeepSeek account has no yuan balance; other currencies are not supported yet",
            ),
        }
        .to_owned()
    }
}

pub struct BalanceResponse {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
}

pub trait BalanceTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        proxy: Option<&str>,
        timeout: Duration,
    ) -> Result<BalanceResponse, BalanceError>;
}

/// Sends only to [`ACCOUNT_URL`] and [`API_KEY_URL`], without following redirects.
pub struct UreqBalanceTransport;

impl BalanceTransport for UreqBalanceTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        proxy: Option<&str>,
        timeout: Duration,
    ) -> Result<BalanceResponse, BalanceError> {
        if url != ACCOUNT_URL && url != API_KEY_URL {
            return Err(BalanceError::Network);
        }
        let mut builder = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .timeout(timeout);
        if let Some(proxy) = proxy {
            if !proxy.starts_with("http://") {
                return Err(BalanceError::Network);
            }
            builder = builder.proxy(ureq::Proxy::new(proxy).map_err(|_| BalanceError::Network)?);
        }
        let mut request = builder.build().get(url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        let response = match request.call() {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(_) => return Err(BalanceError::Network),
        };
        let status = response.status();
        let mut body = Zeroizing::new(Vec::new());
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| BalanceError::Network)?;
        if body.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(BalanceError::Parse);
        }
        Ok(BalanceResponse { status, body })
    }
}

/// How the request identifies its client. Platform requires these five headers;
/// the values describe this app, not Harness.
pub struct ClientIdentity {
    pub windows: bool,
    pub version: String,
    pub chinese: bool,
    pub utc_offset_seconds: i32,
}

pub struct BalanceOptions {
    pub credential: DeepSeekCredential,
    pub proxy: Option<String>,
    pub timeout: Duration,
    pub client: ClientIdentity,
}

/// One scalar from a plain or quoted YAML value, or None for an empty or nested one.
fn scalar(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(value);
    (!value.is_empty()).then_some(value)
}

/// Reads `path.to.key` from the block mappings Harness writes. Only plain nested
/// mappings are understood; anything else reads as absent rather than guessed.
fn yaml_value(document: &str, path: &[&str]) -> Option<Zeroizing<String>> {
    let mut depth = 0;
    // Indent of the last key matched, and of its direct children once seen.
    let mut parent: Option<usize> = None;
    let mut child: Option<usize> = None;
    for line in document.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if parent.is_some_and(|parent| indent <= parent) {
            // Left the mapping being followed without finding the key.
            return None;
        }
        match child {
            None => child = Some(indent),
            Some(child) if child != indent => continue,
            Some(_) => {}
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if scalar(key) != Some(path[depth]) {
            continue;
        }
        if depth + 1 == path.len() {
            return scalar(value).map(|value| Zeroizing::new(value.to_owned()));
        }
        parent = Some(indent);
        child = None;
        depth += 1;
    }
    None
}

fn credentials(root: &Path) -> Option<Zeroizing<String>> {
    fs::read(root.join(".credentials.yaml"))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(Zeroizing::new)
}

/// The account token, only when it was issued by DeepSeek's own platform.
#[must_use]
pub fn read_account_token(root: &Path) -> Option<Zeroizing<String>> {
    let document = credentials(root)?;
    let record = ["records", "deepseek-account-platform/default", "payload"];
    let issuer = yaml_value(&document, &[record[0], record[1], record[2], "issuer"])?;
    if issuer.trim_end_matches('/') != ACCOUNT_ORIGIN {
        return None;
    }
    yaml_value(&document, &[record[0], record[1], record[2], "token"])
}

/// The API key in Harness's order: its credential file, the launch environment,
/// then `$DSH_HOME/.env`. A GUI app usually does not inherit a shell's variables.
#[must_use]
pub fn read_api_key(root: &Path, environment: Option<&OsStr>) -> Option<Zeroizing<String>> {
    if let Some(key) =
        credentials(root).and_then(|document| yaml_value(&document, &["refs", "DEEPSEEK_API_KEY"]))
    {
        return Some(key);
    }
    if let Some(key) = environment
        .and_then(OsStr::to_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        return Some(Zeroizing::new(key.to_owned()));
    }
    let dotenv = Zeroizing::new(fs::read_to_string(root.join(".env")).ok()?);
    dotenv.lines().find_map(|line| {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        let (name, value) = line.split_once('=')?;
        (name.trim() == "DEEPSEEK_API_KEY")
            .then(|| scalar(value).map(|value| Zeroizing::new(value.to_owned())))
            .flatten()
    })
}

fn amount(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

#[derive(Deserialize)]
struct AccountEnvelope {
    code: i64,
    data: Option<AccountData>,
}
#[derive(Deserialize)]
struct AccountData {
    biz_code: i64,
    biz_data: Option<AccountSummary>,
}
#[derive(Deserialize)]
struct AccountSummary {
    normal_wallets: Vec<Wallet>,
    bonus_wallets: Vec<Wallet>,
}
#[derive(Deserialize)]
struct Wallet {
    currency: String,
    balance: String,
}

fn parse_account(body: &[u8], now: DateTime<Utc>) -> Result<DeepSeekBalance, BalanceError> {
    let envelope: AccountEnvelope =
        serde_json::from_slice(body).map_err(|_| BalanceError::Parse)?;
    if envelope.code == ACCOUNT_UNAUTHORIZED {
        return Err(BalanceError::Unauthorized(DeepSeekRoute::Account));
    }
    let summary = envelope
        .data
        .filter(|data| envelope.code == 0 && data.biz_code == 0)
        .and_then(|data| data.biz_data)
        .ok_or(BalanceError::Parse)?;
    let sum = |wallets: &[Wallet]| -> Result<Option<f64>, BalanceError> {
        wallets
            .iter()
            .filter(|wallet| wallet.currency == "CNY")
            .try_fold(None, |total: Option<f64>, wallet| {
                let value = amount(&wallet.balance).ok_or(BalanceError::Parse)?;
                Ok(Some(total.unwrap_or(0.0) + value))
            })
    };
    let topped_up = sum(&summary.normal_wallets)?;
    let granted = sum(&summary.bonus_wallets)?;
    if topped_up.is_none() && granted.is_none() {
        let any = !summary.normal_wallets.is_empty() || !summary.bonus_wallets.is_empty();
        return if any {
            Err(BalanceError::Currency)
        } else {
            Ok(official(0.0, 0.0, now))
        };
    }
    Ok(official(
        topped_up.unwrap_or(0.0),
        granted.unwrap_or(0.0),
        now,
    ))
}

#[derive(Deserialize)]
struct ApiBalance {
    balance_infos: Vec<ApiBalanceInfo>,
}
#[derive(Deserialize)]
struct ApiBalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

fn parse_api_key(body: &[u8], now: DateTime<Utc>) -> Result<DeepSeekBalance, BalanceError> {
    let parsed: ApiBalance = serde_json::from_slice(body).map_err(|_| BalanceError::Parse)?;
    let Some(info) = parsed
        .balance_infos
        .iter()
        .find(|info| info.currency == "CNY")
    else {
        return if parsed.balance_infos.is_empty() {
            Ok(official(0.0, 0.0, now))
        } else {
            Err(BalanceError::Currency)
        };
    };
    let (Some(total), Some(granted), Some(topped_up)) = (
        amount(&info.total_balance),
        amount(&info.granted_balance),
        amount(&info.topped_up_balance),
    ) else {
        return Err(BalanceError::Parse);
    };
    let mut balance = official(topped_up, granted, now);
    balance.total = total;
    Ok(balance)
}

fn official(topped_up: f64, granted: f64, now: DateTime<Utc>) -> DeepSeekBalance {
    DeepSeekBalance {
        total: topped_up + granted,
        topped_up: Some(topped_up),
        granted: Some(granted),
        origin: BalanceOrigin::Official,
        as_of: now,
        spent_since: None,
    }
}

fn client_headers(client: &ClientIdentity) -> [(&'static str, String); 5] {
    [
        ("x-client-bundle-id", String::new()),
        (
            "x-client-platform",
            if client.windows {
                "desktop-win"
            } else {
                "desktop-mac"
            }
            .to_owned(),
        ),
        ("x-client-version", client.version.clone()),
        (
            "x-client-locale",
            if client.chinese { "zh_CN" } else { "en_US" }.to_owned(),
        ),
        (
            "x-client-timezone-offset",
            client.utc_offset_seconds.to_string(),
        ),
    ]
}

fn query_account(
    token: &str,
    transport: &impl BalanceTransport,
    options: &BalanceOptions,
    now: DateTime<Utc>,
) -> Result<DeepSeekBalance, BalanceError> {
    let client = client_headers(&options.client);
    let mut headers: Vec<(&str, &str)> = client
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    headers.push(("x-dsh-auth-token", token));
    let response = transport.get(
        ACCOUNT_URL,
        &headers,
        options.proxy.as_deref(),
        options.timeout,
    )?;
    match response.status {
        200 => parse_account(&response.body, now),
        401 => Err(BalanceError::Unauthorized(DeepSeekRoute::Account)),
        _ => Err(BalanceError::Network),
    }
}

fn query_api_key(
    key: &str,
    transport: &impl BalanceTransport,
    options: &BalanceOptions,
    now: DateTime<Utc>,
) -> Result<DeepSeekBalance, BalanceError> {
    let authorization = Zeroizing::new(format!("Bearer {key}"));
    let headers = [
        ("Authorization", authorization.as_str()),
        ("Accept", "application/json"),
    ];
    let response = transport.get(
        API_KEY_URL,
        &headers,
        options.proxy.as_deref(),
        options.timeout,
    )?;
    match response.status {
        200 => parse_api_key(&response.body, now),
        401 | 403 => Err(BalanceError::Unauthorized(DeepSeekRoute::ApiKey)),
        _ => Err(BalanceError::Network),
    }
}

/// Queries the wallet with the login `options.credential` picks. `environment` is the
/// value of `DEEPSEEK_API_KEY`, passed in so tests never read the real one.
pub fn query_deepseek_balance(
    root: &Path,
    environment: Option<&OsStr>,
    transport: &impl BalanceTransport,
    options: &BalanceOptions,
    now: DateTime<Utc>,
) -> Result<DeepSeekBalance, BalanceError> {
    let account = || read_account_token(root);
    let api_key = || read_api_key(root, environment);
    match options.credential {
        DeepSeekCredential::Account => {
            let token = account().ok_or(BalanceError::NoCredential)?;
            query_account(&token, transport, options, now)
        }
        DeepSeekCredential::ApiKey => {
            let key = api_key().ok_or(BalanceError::NoCredential)?;
            query_api_key(&key, transport, options, now)
        }
        DeepSeekCredential::Auto => {
            if let Some(token) = account() {
                query_account(&token, transport, options, now)
            } else if let Some(key) = api_key() {
                query_api_key(&key, transport, options, now)
            } else {
                Err(BalanceError::NoCredential)
            }
        }
    }
}
