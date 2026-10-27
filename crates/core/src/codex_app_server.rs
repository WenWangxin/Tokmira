use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use crate::codex::RateLimitWindow;

const MAX_LINE: usize = 4 * 1024 * 1024;
pub const ALLOWED_METHODS: [&str; 4] = [
    "initialize",
    "initialized",
    "account/rateLimits/read",
    "account/usage/read",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl Launch {
    fn new(program: PathBuf, args: Vec<OsString>) -> Self {
        Self { program, args }
    }
}

fn add_candidate(path: PathBuf, candidates: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    if !path.is_file() {
        return;
    }
    let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if !seen.contains(&identity) {
        seen.push(identity);
        candidates.push(path);
    }
}

fn add_directory(directory: &Path, candidates: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>) {
    #[cfg(windows)]
    let names = ["codex.exe", "codex.cmd"];
    #[cfg(not(windows))]
    let names = ["codex"];
    for name in names {
        add_candidate(directory.join(name), candidates, seen);
    }
}

fn node_version(name: &OsStr) -> Vec<u64> {
    name.to_string_lossy()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// Finds existing Codex launchers without consulting the process environment.
pub fn codex_candidates(home: &Path, path_env: Option<&OsStr>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = Vec::new();
    if let Some(path_env) = path_env {
        for directory in std::env::split_paths(path_env) {
            add_directory(&directory, &mut candidates, &mut seen);
        }
    }
    #[cfg(target_os = "macos")]
    for directory in [Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")] {
        add_directory(directory, &mut candidates, &mut seen);
    }
    for relative in [".local/bin", ".npm-global/bin", ".volta/bin"] {
        add_directory(&home.join(relative), &mut candidates, &mut seen);
    }
    let nvm = home.join(".nvm/versions/node");
    if let Ok(entries) = fs::read_dir(nvm) {
        let mut versions: Vec<_> = entries.filter_map(Result::ok).collect();
        versions.sort_by(|a, b| {
            node_version(&b.file_name())
                .cmp(&node_version(&a.file_name()))
                .then_with(|| b.file_name().cmp(&a.file_name()))
        });
        for entry in versions {
            add_directory(&entry.path().join("bin"), &mut candidates, &mut seen);
        }
    }
    #[cfg(windows)]
    add_candidate(
        home.join("AppData/Roaming/npm/codex.cmd"),
        &mut candidates,
        &mut seen,
    );
    #[cfg(target_os = "macos")]
    add_candidate(
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex"),
        &mut candidates,
        &mut seen,
    );
    candidates
}

fn is_node_script(path: &Path) -> bool {
    if path.extension().is_some_and(|extension| extension == "js") {
        return true;
    }
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut first = String::new();
    BufReader::new(file)
        .take(128)
        .read_line(&mut first)
        .is_ok_and(|_| first.starts_with("#!/usr/bin/env node"))
}

fn native_from_package(script: &Path) -> Option<PathBuf> {
    let package = script.parent()?.parent()?;
    if package.file_name()? != "codex" || package.parent()?.file_name()? != "@openai" {
        return None;
    }
    let packages = package.join("node_modules/@openai");
    let mut entries: Vec<_> = fs::read_dir(packages)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("codex-"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let target = "aarch64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let target = "x86_64-apple-darwin";
    #[cfg(all(windows, target_arch = "aarch64"))]
    let target = "aarch64-pc-windows-msvc";
    #[cfg(all(windows, target_arch = "x86_64"))]
    let target = "x86_64-pc-windows-msvc";
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(windows, target_arch = "aarch64"),
        all(windows, target_arch = "x86_64")
    )))]
    let target = "";
    for entry in entries {
        let vendor = entry.path().join("vendor");
        let Ok(targets) = fs::read_dir(vendor) else {
            continue;
        };
        for triple in targets.filter_map(Result::ok) {
            if !target.is_empty() && triple.file_name() != target {
                continue;
            }
            #[cfg(windows)]
            let binary = "codex.exe";
            #[cfg(not(windows))]
            let binary = "codex";
            for relative in [format!("bin/{binary}"), format!("codex/{binary}")] {
                let path = triple.path().join(relative);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Resolves npm launchers to the bundled native executable when possible.
pub fn resolve_launch(candidate: &Path) -> Launch {
    #[cfg(windows)]
    if candidate
        .extension()
        .is_some_and(|extension| extension == "cmd")
    {
        return Launch::new(
            PathBuf::from("cmd"),
            vec![OsString::from("/C"), candidate.as_os_str().to_owned()],
        );
    }
    let resolved = fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_owned());
    if is_node_script(&resolved) {
        if let Some(native) = native_from_package(&resolved) {
            return Launch::new(native, Vec::new());
        }
        #[cfg(windows)]
        let node = "node.exe";
        #[cfg(not(windows))]
        let node = "node";
        let directory = candidate.parent().unwrap_or_else(|| Path::new("."));
        let program = fs::canonicalize(directory)
            .unwrap_or_else(|_| directory.to_owned())
            .join(node);
        return Launch::new(program, vec![resolved.into_os_string()]);
    }
    Launch::new(resolved, Vec::new())
}

#[derive(Clone, Debug)]
pub struct AppServerOptions {
    pub timeout: Duration,
}

impl Default for AppServerOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexQuotaError {
    NotFound,
    Spawn,
    Timeout,
    Rpc { code: i64, message: String },
    Protocol,
    Parse,
}

impl CodexQuotaError {
    pub fn message(&self, lang: crate::i18n::Lang) -> String {
        use crate::tr;
        match self {
            Self::NotFound => lang
                .pick("未找到 Codex 可执行文件", "Codex executable not found")
                .to_owned(),
            Self::Spawn => lang
                .pick(
                    "无法启动 Codex app-server",
                    "Could not start Codex app-server",
                )
                .to_owned(),
            Self::Timeout => lang
                .pick("Codex app-server 响应超时", "Codex app-server timed out")
                .to_owned(),
            Self::Rpc { code, message } => tr!(
                lang,
                "Codex app-server 错误 {code}：{message}",
                "Codex app-server error {code}: {message}"
            ),
            Self::Protocol => lang
                .pick(
                    "Codex app-server 通信格式异常",
                    "Unexpected Codex app-server response format",
                )
                .to_owned(),
            Self::Parse => lang
                .pick(
                    "Codex app-server 额度数据格式异常",
                    "Unexpected Codex app-server quota data format",
                )
                .to_owned(),
        }
    }
}

impl std::fmt::Display for CodexQuotaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message(crate::i18n::Lang::Zh))
    }
}

impl std::error::Error for CodexQuotaError {}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CodexQuota {
    pub fetched_at: DateTime<Utc>,
    pub ordinary_usage_allowed: Option<bool>,
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
    pub rate_limit_reached_type: Option<String>,
    pub credits: Option<CodexCredits>,
    pub other_limits: Vec<CodexLimit>,
    pub reset_credits: Option<ResetCredits>,
    pub usage: Option<CodexAccountUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodexCredits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CodexLimit {
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub primary: Option<RateLimitWindow>,
    pub secondary: Option<RateLimitWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResetCredits {
    pub available_count: Option<u64>,
    pub credits: Vec<ResetCredit>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResetCredit {
    pub reset_type: Option<String>,
    pub status: Option<String>,
    pub title: Option<String>,
    pub granted_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CodexAccountUsage {
    pub lifetime_tokens: Option<u64>,
    pub peak_daily_tokens: Option<u64>,
    pub daily: Vec<DailyUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DailyUsage {
    pub date: NaiveDate,
    pub tokens: u64,
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

fn time_field(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(value.get(key)?.as_i64()?, 0).single()
}

fn window(value: Option<&Value>) -> Result<Option<RateLimitWindow>, CodexQuotaError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let used_percent = value
        .get("usedPercent")
        .and_then(Value::as_f64)
        .ok_or(CodexQuotaError::Parse)?;
    if !used_percent.is_finite() {
        return Err(CodexQuotaError::Parse);
    }
    Ok(Some(RateLimitWindow {
        used_percent,
        window_minutes: value.get("windowDurationMins").and_then(Value::as_u64),
        resets_at: time_field(value, "resetsAt"),
    }))
}

fn limit(value: &Value) -> Result<CodexLimit, CodexQuotaError> {
    if !value.is_object() {
        return Err(CodexQuotaError::Parse);
    }
    Ok(CodexLimit {
        limit_id: str_field(value, "limitId"),
        plan_type: str_field(value, "planType"),
        primary: window(value.get("primary"))?,
        secondary: window(value.get("secondary"))?,
    })
}

/// Parses only the fields needed for quota display; unrelated account fields are ignored.
pub fn parse_codex_quota_at(
    rate_result: &Value,
    usage_result: Option<&Value>,
    fetched_at: DateTime<Utc>,
) -> Result<CodexQuota, CodexQuotaError> {
    if !rate_result.is_object() {
        return Err(CodexQuotaError::Parse);
    }
    let main = rate_result
        .get("rateLimits")
        .filter(|value| !value.is_null());
    let main_limit = main.map(limit).transpose()?;
    let mut other_limits = Vec::new();
    if let Some(by_id) = rate_result
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
    {
        for (id, value) in by_id {
            if main_limit
                .as_ref()
                .and_then(|main| main.limit_id.as_deref())
                == Some(id.as_str())
            {
                continue;
            }
            other_limits.push(limit(value)?);
        }
    }
    let credits = main
        .and_then(|value| value.get("credits"))
        .and_then(|value| {
            Some(CodexCredits {
                has_credits: value.get("hasCredits")?.as_bool()?,
                unlimited: value.get("unlimited")?.as_bool()?,
                balance: str_field(value, "balance")?,
            })
        });
    let reset_credits = rate_result
        .get("rateLimitResetCredits")
        .filter(|value| !value.is_null())
        .map(|value| ResetCredits {
            available_count: value.get("availableCount").and_then(Value::as_u64),
            credits: value
                .get("credits")
                .and_then(Value::as_array)
                .map(|cards| {
                    cards
                        .iter()
                        .map(|card| ResetCredit {
                            reset_type: str_field(card, "resetType"),
                            status: str_field(card, "status"),
                            title: str_field(card, "title"),
                            granted_at: time_field(card, "grantedAt"),
                            expires_at: time_field(card, "expiresAt"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });
    let usage = usage_result.and_then(|result| {
        let summary = result.get("summary")?;
        let daily = result
            .get("dailyUsageBuckets")
            .and_then(Value::as_array)
            .map(|buckets| {
                buckets
                    .iter()
                    .filter_map(|bucket| {
                        Some(DailyUsage {
                            date: NaiveDate::parse_from_str(
                                bucket.get("startDate")?.as_str()?,
                                "%Y-%m-%d",
                            )
                            .ok()?,
                            tokens: bucket.get("tokens")?.as_u64()?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(CodexAccountUsage {
            lifetime_tokens: summary.get("lifetimeTokens").and_then(Value::as_u64),
            peak_daily_tokens: summary.get("peakDailyTokens").and_then(Value::as_u64),
            daily,
        })
    });
    Ok(CodexQuota {
        fetched_at,
        ordinary_usage_allowed: rate_result
            .get("ordinaryUsageAllowed")
            .and_then(Value::as_bool),
        limit_id: main_limit.as_ref().and_then(|limit| limit.limit_id.clone()),
        plan_type: main_limit
            .as_ref()
            .and_then(|limit| limit.plan_type.clone()),
        primary: main_limit.as_ref().and_then(|limit| limit.primary.clone()),
        secondary: main_limit
            .as_ref()
            .and_then(|limit| limit.secondary.clone()),
        rate_limit_reached_type: main.and_then(|value| str_field(value, "rateLimitReachedType")),
        credits,
        other_limits,
        reset_credits,
        usage,
    })
}

fn read_line_limited(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete line",
                ))
            };
        }
        let length = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len() + length > MAX_LINE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
        line.extend_from_slice(&available[..length]);
        reader.consume(length);
        if line.last() == Some(&b'\n') {
            return Ok(Some(line));
        }
    }
}

fn start_reader(
    stdout: impl io::Read + Send + 'static,
) -> Receiver<Result<Vec<u8>, CodexQuotaError>> {
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_line_limited(&mut reader) {
                Ok(Some(line)) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = sender.send(Err(CodexQuotaError::Protocol));
                    break;
                }
            }
        }
    });
    receiver
}

fn send(
    child: &mut Child,
    method: &str,
    id: Option<u64>,
    params: Option<Value>,
) -> Result<(), CodexQuotaError> {
    if !ALLOWED_METHODS.contains(&method) {
        return Err(CodexQuotaError::Protocol);
    }
    let mut request = json!({"method": method});
    if let Some(id) = id {
        request["id"] = json!(id);
    }
    if let Some(params) = params {
        request["params"] = params;
    }
    let stdin = child.stdin.as_mut().ok_or(CodexQuotaError::Protocol)?;
    serde_json::to_writer(&mut *stdin, &request).map_err(|_| CodexQuotaError::Protocol)?;
    stdin
        .write_all(b"\n")
        .map_err(|_| CodexQuotaError::Protocol)?;
    stdin.flush().map_err(|_| CodexQuotaError::Protocol)
}

fn wait_for(
    receiver: &Receiver<Result<Vec<u8>, CodexQuotaError>>,
    id: u64,
    deadline: Instant,
) -> Result<Value, CodexQuotaError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CodexQuotaError::Timeout);
        }
        let line = receiver
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => CodexQuotaError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => CodexQuotaError::Protocol,
            })??;
        let Ok(response) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if response.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message: String = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .chars()
                .take(200)
                .collect();
            return Err(CodexQuotaError::Rpc { code, message });
        }
        return response
            .get("result")
            .cloned()
            .ok_or(CodexQuotaError::Protocol);
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => break,
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads the account quota through the installed Codex local app-server.
pub fn read_codex_quota(
    launch: &Launch,
    options: &AppServerOptions,
) -> Result<CodexQuota, CodexQuotaError> {
    if !launch.program.is_file() && launch.program != Path::new("cmd") {
        return Err(CodexQuotaError::NotFound);
    }
    let deadline = Instant::now() + options.timeout;
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = command.spawn().map_err(|_| CodexQuotaError::Spawn)?;
    let mut child = ChildGuard(child);
    let stdout = child.0.stdout.take().ok_or(CodexQuotaError::Protocol)?;
    let receiver = start_reader(stdout);
    send(
        &mut child.0,
        "initialize",
        Some(1),
        Some(json!({"clientInfo": {"name": "token-audit", "version": crate::version()}})),
    )?;
    wait_for(&receiver, 1, deadline)?;
    send(&mut child.0, "initialized", None, None)?;
    send(
        &mut child.0,
        "account/rateLimits/read",
        Some(2),
        Some(json!({})),
    )?;
    let rate_result = wait_for(&receiver, 2, deadline)?;
    let usage_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| Utc.timestamp_opt(duration.as_secs() as i64, 0).single())
        .ok_or(CodexQuotaError::Parse)?;
    let usage_result = if send(&mut child.0, "account/usage/read", Some(3), Some(json!({}))).is_ok()
    {
        wait_for(&receiver, 3, usage_deadline).ok()
    } else {
        None
    };
    parse_codex_quota_at(&rate_result, usage_result.as_ref(), fetched_at)
}
