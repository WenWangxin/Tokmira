use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use serde_json::Value;
use token_audit_core::codex_app_server::{codex_candidates, parse_codex_quota_at, ALLOWED_METHODS};
#[cfg(unix)]
use token_audit_core::codex_app_server::{
    read_codex_quota, resolve_launch, AppServerOptions, CodexQuotaError,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-app-server-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temp directory");
    }
}

fn fixtures() -> (Value, Value) {
    (
        serde_json::from_str(include_str!("fixtures/codex_app_server/rate_full.json"))
            .expect("handwritten rate fixture"),
        serde_json::from_str(include_str!("fixtures/codex_app_server/usage_full.json"))
            .expect("handwritten usage fixture"),
    )
}

fn parsed(rate: &Value, usage: Option<&Value>) -> token_audit_core::codex_app_server::CodexQuota {
    parse_codex_quota_at(
        rate,
        usage,
        Utc.timestamp_opt(1792800000, 0)
            .single()
            .expect("timestamp"),
    )
    .expect("parse quota")
}

#[test]
fn parses_full_response_without_account_identifiers_or_descriptions() {
    let (rate, usage) = fixtures();
    let quota = parsed(&rate, Some(&usage));
    assert_eq!(quota.limit_id.as_deref(), Some("codex"));
    assert_eq!(quota.plan_type.as_deref(), Some("plus"));
    assert_eq!(
        quota.primary.as_ref().map(|window| window.used_percent),
        Some(30.0)
    );
    assert_eq!(
        quota
            .primary
            .as_ref()
            .and_then(|window| window.window_minutes),
        Some(300)
    );
    assert_eq!(
        quota.secondary.as_ref().map(|window| window.used_percent),
        Some(9.0)
    );
    assert_eq!(
        quota
            .credits
            .as_ref()
            .map(|credits| credits.balance.as_str()),
        Some("0")
    );
    assert_eq!(quota.other_limits.len(), 1);
    assert_eq!(quota.other_limits[0].limit_id.as_deref(), Some("review"));
    assert_eq!(
        quota
            .reset_credits
            .as_ref()
            .and_then(|cards| cards.available_count),
        Some(1)
    );
    assert_eq!(
        quota.usage.as_ref().and_then(|usage| usage.lifetime_tokens),
        Some(1000)
    );
    assert_eq!(quota.usage.as_ref().map(|usage| usage.daily.len()), Some(2));
    let json = serde_json::to_string(&quota).expect("serialize quota");
    for private in [
        "accountId",
        "userId",
        "ignored-account-id",
        "description",
        "threadUsage",
    ] {
        assert!(!json.contains(private));
    }
}

#[test]
fn missing_optional_fields_are_preserved_as_none() {
    let (mut rate, mut usage) = fixtures();
    rate["rateLimits"]["secondary"] = Value::Null;
    rate["rateLimitResetCredits"] = Value::Null;
    usage["summary"] = serde_json::json!({});
    let quota = parsed(&rate, Some(&usage));
    assert!(quota.secondary.is_none());
    assert!(quota.reset_credits.is_none());
    assert_eq!(
        quota.usage.as_ref().and_then(|usage| usage.lifetime_tokens),
        None
    );
    rate["rateLimits"] = Value::Null;
    let quota = parsed(&rate, None);
    assert!(quota.primary.is_none());
    assert!(quota.credits.is_none());
    assert_eq!(quota.other_limits.len(), 2);
    assert!(quota.usage.is_none());
}

#[test]
fn only_read_methods_are_allowlisted() {
    assert_eq!(ALLOWED_METHODS.len(), 4);
    assert!(!ALLOWED_METHODS.contains(&"account/rateLimitResetCredit/consume"));
    assert!(!ALLOWED_METHODS.contains(&"account/logout"));
    assert!(!ALLOWED_METHODS.contains(&"account/login/start"));
}

#[test]
fn candidate_search_orders_numeric_node_versions_and_deduplicates() {
    let temp = TempDir::new();
    #[cfg(windows)]
    let binary = "codex.exe";
    #[cfg(not(windows))]
    let binary = "codex";
    let path_dir = temp.path().join("path-bin");
    fs::create_dir(&path_dir).expect("path directory");
    fs::write(path_dir.join(binary), "fixture").expect("path codex");
    for version in ["v9.0.0", "v10.2.0", "v20.0.0"] {
        let bin = temp
            .path()
            .join(".nvm/versions/node")
            .join(version)
            .join("bin");
        fs::create_dir_all(&bin).expect("nvm directory");
        fs::write(bin.join(binary), "fixture").expect("nvm codex");
    }
    let candidates = codex_candidates(temp.path(), Some(path_dir.as_os_str()));
    assert_eq!(candidates[0], path_dir.join(binary));
    let nvm: Vec<_> = candidates
        .iter()
        .filter(|path| path.to_string_lossy().contains(".nvm"))
        .collect();
    assert_eq!(nvm.len(), 3);
    assert!(nvm[0].to_string_lossy().contains("v20.0.0"));
    assert!(nvm[1].to_string_lossy().contains("v10.2.0"));
    assert!(nvm[2].to_string_lossy().contains("v9.0.0"));
    let paths = std::env::join_paths([path_dir.as_os_str(), path_dir.as_os_str()]).expect("PATH");
    assert_eq!(
        codex_candidates(temp.path(), Some(&paths))[0],
        path_dir.join(binary)
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::process::Command;
    use std::thread;

    fn script(temp: &TempDir, body: &str) -> PathBuf {
        let path = temp.path().join("codex");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("chmod script");
        path
    }

    fn read(
        path: &Path,
        timeout: Duration,
    ) -> Result<token_audit_core::codex_app_server::CodexQuota, CodexQuotaError> {
        read_codex_quota(&resolve_launch(path), &AppServerOptions { timeout })
    }

    #[test]
    fn resolves_symlinked_npm_launcher_to_native_and_falls_back_to_sibling_node() {
        let temp = TempDir::new();
        let script_dir = temp.path().join("lib/node_modules/@openai/codex/bin");
        fs::create_dir_all(&script_dir).expect("npm bin");
        let js = script_dir.join("codex.js");
        fs::write(&js, "#!/usr/bin/env node\n").expect("npm script");
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).expect("bin");
        let link = bin.join("codex");
        symlink(&js, &link).expect("symlink");
        let fallback = resolve_launch(&link);
        assert_eq!(
            fallback.program,
            fs::canonicalize(&bin).expect("canonical bin").join("node")
        );
        assert_eq!(
            fallback.args,
            vec![fs::canonicalize(&js)
                .expect("canonical npm script")
                .into_os_string()]
        );
        let package = temp
            .path()
            .join("lib/node_modules/@openai/codex/node_modules/@openai/codex-fixture/vendor");
        let target = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("macos", "x86_64") => "x86_64-apple-darwin",
            ("windows", "aarch64") => "aarch64-pc-windows-msvc",
            ("windows", "x86_64") => "x86_64-pc-windows-msvc",
            _ => "fixture-target",
        };
        let native = package.join(target).join("bin/codex");
        fs::create_dir_all(native.parent().expect("native parent")).expect("vendor");
        fs::write(&native, "fixture").expect("native");
        assert_eq!(
            resolve_launch(&link).program,
            fs::canonicalize(native).expect("canonical native")
        );
        let old = package.join(target).join("codex/codex");
        fs::create_dir_all(old.parent().expect("old native parent")).expect("old vendor");
        fs::write(&old, "fixture").expect("old native");
        fs::remove_file(package.join(target).join("bin/codex")).expect("remove new layout");
        assert_eq!(
            resolve_launch(&link).program,
            fs::canonicalize(old).expect("canonical old native")
        );
    }

    #[test]
    fn reads_quota_with_interleaved_notification() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
printf '%s\n' '{"method":"notice"}' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":99,"result":{}}' '{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","planType":"plus","primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":1792788614}}}}'
read -r fourth
printf '%s\n' '{"id":3,"result":{"summary":{"lifetimeTokens":100},"dailyUsageBuckets":[{"startDate":"2026-10-20","tokens":30}]}}'
"#,
        );
        let quota = read(&path, Duration::from_secs(2)).expect("quota");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(30.0)
        );
        assert_eq!(
            quota.usage.as_ref().and_then(|usage| usage.lifetime_tokens),
            Some(100)
        );
    }

    #[test]
    fn skips_non_json_stdout_before_response() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
printf '%s\n' 'Codex is starting' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12}}}}'
read -r fourth
printf '%s\n' '{"id":3,"result":{}}'
"#,
        );
        let quota = read(&path, Duration::from_secs(2)).expect("quota after stdout notice");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(12.0)
        );
    }

    #[test]
    fn usage_error_does_not_discard_quota() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
printf '%s\n' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":5}}}}'
read -r fourth
printf '%s\n' '{"id":3,"error":{"code":-1,"message":"usage unavailable"}}'
"#,
        );
        let quota = read(&path, Duration::from_secs(2)).expect("quota survives usage error");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(5.0)
        );
        assert!(quota.usage.is_none());
    }

    #[test]
    fn usage_timeout_keeps_quota() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
printf '%s\n' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":7}}}}'
read -r fourth
exec sleep 10
"#,
        );
        let quota = read(&path, Duration::from_secs(2)).expect("quota survives usage timeout");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(7.0)
        );
        assert!(quota.usage.is_none());
    }

    #[test]
    fn usage_wait_is_capped_after_quota_arrives() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
printf '%s\n' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":8}}}}'
read -r fourth
while read -r more; do :; done
"#,
        );
        let start = Instant::now();
        let quota = read(&path, Duration::from_secs(15)).expect("quota survives slow usage");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(8.0)
        );
        assert!(quota.usage.is_none());
        assert!(start.elapsed() < Duration::from_secs(9));
    }

    #[test]
    fn closing_stdin_exits_wrapped_server() {
        let temp = TempDir::new();
        let pid_path = temp.path().join("server.pid");
        let server = temp.path().join("server.sh");
        fs::write(
            &server,
            format!(
                "#!/bin/sh\necho $$ > '{}'\nread -r first\nprintf '%s\\n' '{{\"id\":1,\"result\":{{}}}}'\nread -r second\nread -r third\nprintf '%s\\n' '{{\"id\":2,\"result\":{{\"rateLimits\":{{\"primary\":{{\"usedPercent\":9}}}}}}}}'\nread -r fourth\nprintf '%s\\n' '{{\"id\":3,\"result\":{{}}}}'\nwhile read -r more; do :; done\n",
                pid_path.display()
            ),
        )
        .expect("write wrapped server");
        fs::set_permissions(&server, fs::Permissions::from_mode(0o700)).expect("chmod server");
        let wrapper = script(&temp, &format!("'{}'\nexit 0", server.display()));
        let quota = read(&wrapper, Duration::from_secs(2)).expect("wrapped quota");
        assert_eq!(
            quota.primary.as_ref().map(|window| window.used_percent),
            Some(9.0)
        );
        let pid = fs::read_to_string(pid_path).expect("server pid");
        assert!(!Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill -0")
            .success());
    }

    #[test]
    fn rejects_oversized_protocol_line() {
        let temp = TempDir::new();
        let path = script(
            &temp,
            r#"
read -r first
head -c 4194305 /dev/zero | tr '\000' x
printf '\n'
"#,
        );
        assert_eq!(
            read(&path, Duration::from_secs(3)),
            Err(CodexQuotaError::Protocol)
        );
    }

    #[test]
    fn timeout_kills_child_and_immediate_exit_does_not_hang() {
        let temp = TempDir::new();
        let pid_path = temp.path().join("pid");
        let path = script(
            &temp,
            &format!("echo $$ > '{}'\nexec sleep 10", pid_path.display()),
        );
        assert_eq!(
            read(&path, Duration::from_secs(2)),
            Err(CodexQuotaError::Timeout)
        );
        let pid = fs::read_to_string(&pid_path).expect("child pid");
        thread::sleep(Duration::from_millis(30));
        assert!(!Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill -0")
            .success());
        script(&temp, "exit 0");
        assert!(matches!(
            read(&path, Duration::from_secs(1)),
            Err(CodexQuotaError::Protocol)
        ));
    }
}
