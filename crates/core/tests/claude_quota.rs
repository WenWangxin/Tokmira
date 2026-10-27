use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use aes::Aes128;
#[cfg(target_os = "macos")]
use base64::Engine;
#[cfg(target_os = "macos")]
use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use chrono::Utc;
#[cfg(target_os = "macos")]
use token_audit_core::claude_quota::SystemSecretStore;
use token_audit_core::claude_quota::{
    parse_scutil_proxy, parse_windows_proxy, query_claude_quota, ClaudeQuotaError,
    CredentialPreference, CredentialSource, QueryOptions, SecretError, SecretStore, UsageResponse,
    UsageTransport, USAGE_URL,
};
use zeroize::Zeroizing;

const CODE: &str = include_str!("fixtures/claude_quota/code.json");
const USAGE: &str = include_str!("fixtures/claude_quota/usage.json");
const USAGE_BREAKDOWN: &str = include_str!("fixtures/claude_quota/usage_breakdown.json");

struct TempHome(PathBuf);
static NEXT_HOME: AtomicUsize = AtomicUsize::new(0);

impl TempHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-m6-{}-{}",
            std::process::id(),
            NEXT_HOME.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temporary home");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write_code(&self, text: &str) {
        let path = self.0.join(".claude/.credentials.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("create code directory");
        fs::write(path, text).expect("write fake code credential");
    }
    #[cfg(target_os = "macos")]
    fn write_desktop(&self, blob: &str) {
        let path = self
            .0
            .join("Library/Application Support/Claude/config.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("create desktop directory");
        fs::write(path, format!(r#"{{"oauth:tokenCacheV2":"{blob}"}}"#))
            .expect("write fake desktop config");
    }
    #[cfg(target_os = "macos")]
    fn security_script(&self, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = self.0.join("security");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake security");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("chmod fake security");
        path
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove fake home");
    }
}

#[derive(Clone)]
struct FakeStore {
    desktop: Option<String>,
    code: Option<String>,
    denied: bool,
}

impl SecretStore for FakeStore {
    fn generic_password(
        &self,
        service: &str,
        account: Option<&str>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        if service == "Claude Safe Storage" {
            assert_eq!(account, Some("Claude"));
            if self.denied {
                return Err(SecretError::Denied);
            }
            return Ok(self
                .desktop
                .as_ref()
                .map(|v| Zeroizing::new(v.as_bytes().to_vec())));
        }
        assert_eq!(service, "Claude Code-credentials");
        assert_eq!(account, None);
        Ok(self
            .code
            .as_ref()
            .map(|v| Zeroizing::new(v.as_bytes().to_vec())))
    }
}

struct FakeTransport {
    calls: AtomicUsize,
    status: u16,
    body: &'static str,
    retry_after: Option<&'static str>,
}

impl FakeTransport {
    fn usage() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            status: 200,
            body: USAGE,
            retry_after: None,
        }
    }
    fn status(status: u16, body: &'static str, retry_after: Option<&'static str>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            status,
            body,
            retry_after,
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl UsageTransport for FakeTransport {
    fn get_usage(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _: Option<&str>,
        timeout: Duration,
    ) -> Result<UsageResponse, ClaudeQuotaError> {
        assert_eq!(url, USAGE_URL);
        assert_eq!(timeout, Duration::from_secs(15));
        assert_eq!(
            headers
                .iter()
                .find(|(key, _)| *key == "anthropic-beta")
                .map(|(_, value)| *value),
            Some("oauth-2025-04-20")
        );
        assert_eq!(
            headers
                .iter()
                .find(|(key, _)| *key == "Accept")
                .map(|(_, value)| *value),
            Some("application/json")
        );
        assert_eq!(
            headers
                .iter()
                .find(|(key, _)| *key == "User-Agent")
                .map(|(_, value)| *value),
            Some(concat!("token-audit/", env!("CARGO_PKG_VERSION")))
        );
        let authorization = headers
            .iter()
            .find(|(key, _)| *key == "Authorization")
            .map(|(_, value)| *value)
            .expect("authorization");
        assert!(
            authorization == "Bearer fake-code-token"
                || authorization == "Bearer fake-access-token-AAA"
        );
        assert_eq!(
            headers
                .iter()
                .filter(|(key, _)| *key == "Authorization")
                .count(),
            1
        );
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(UsageResponse {
            status: self.status,
            body: Zeroizing::new(self.body.as_bytes().to_vec()),
            retry_after: self.retry_after.map(str::to_owned),
        })
    }
}

fn options(preference: CredentialPreference) -> QueryOptions {
    QueryOptions {
        preference,
        ..QueryOptions::default()
    }
}

#[test]
fn code_credential_and_usage_are_filtered() {
    let home = TempHome::new();
    home.write_code(CODE);
    let store = FakeStore {
        desktop: None,
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let transport = FakeTransport::usage();
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect("quota");
    assert_eq!(result.source, CredentialSource::ClaudeCode);
    assert_eq!(result.subscription_type.as_deref(), Some("pro"));
    assert_eq!(
        result
            .windows
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>(),
        ["five_hour", "seven_day"]
    );
    assert_eq!(
        result.windows[0].resets_at.expect("iso time").timestamp(),
        4070908800
    );
    assert_eq!(
        result.windows[1]
            .resets_at
            .expect("millis time")
            .timestamp(),
        4102444800
    );
    assert!(result.weekly_breakdown.is_none());
    let extra = result.extra_usage.as_ref().expect("extra usage");
    assert_eq!(extra.is_enabled, Some(true));
    assert_eq!(extra.monthly_limit, Some(100.0));
    assert_eq!(extra.used_credits, Some(5.5));
    assert_eq!(extra.utilization, Some(5.5));
    let json = serde_json::to_string(&result).expect("serialize result");
    assert!(!json.contains("fake-code-token"));
    assert!(!json.contains("fake@example.test"));
    assert!(!json.contains("fake-refresh-token"));
    assert_eq!(transport.calls(), 1);
}

#[test]
fn official_weekly_breakdown_and_only_known_windows_are_retained() {
    let home = TempHome::new();
    home.write_code(CODE);
    let store = FakeStore {
        desktop: None,
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let transport = FakeTransport::status(200, USAGE_BREAKDOWN, None);
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect("quota with breakdown");
    assert_eq!(
        result
            .windows
            .iter()
            .map(|window| window.name.as_str())
            .collect::<Vec<_>>(),
        ["five_hour", "seven_day"]
    );
    let reset = result.windows[0]
        .resets_at
        .expect("fractional offset reset");
    assert_eq!(reset.timestamp_subsec_micros(), 79021);
    let breakdown = result.weekly_breakdown.expect("weekly breakdown");
    assert_eq!(
        breakdown.as_of.expect("as of").timestamp_subsec_micros(),
        417430
    );
    assert_eq!(
        breakdown
            .window_started_at
            .expect("window start")
            .timestamp_subsec_micros(),
        401885
    );
    assert_eq!(breakdown.rows.len(), 5);
    assert_eq!(breakdown.rows[0].key, "claude_code");
    assert_eq!(breakdown.rows[0].percent, 98.0);
    assert_eq!(breakdown.rows[4].key, "new_surface");
    assert_eq!(
        breakdown.rows[4].display_name.as_deref(),
        Some("New Surface")
    );
    assert_eq!(breakdown.rows[4].percent, 0.0);
}

#[test]
fn null_weekly_breakdown_is_absent() {
    let home = TempHome::new();
    home.write_code(CODE);
    let store = FakeStore {
        desktop: None,
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let transport = FakeTransport::status(
        200,
        r#"{"five_hour":{"utilization":5},"seven_day_breakdown":null}"#,
        None,
    );
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect("null breakdown");
    assert!(result.weekly_breakdown.is_none());
}

#[test]
fn expired_and_near_expired_never_call_transport() {
    let home = TempHome::new();
    let store = FakeStore {
        desktop: None,
        code: None,
        denied: false,
    };
    let transport = FakeTransport::usage();
    for expiry in [1_000_i64, Utc::now().timestamp_millis() + 30_000] {
        let code = CODE.replace("4102444800000", &expiry.to_string());
        home.write_code(&code);
        #[cfg(target_os = "macos")]
        let store = FakeStore {
            code: Some(code.clone()),
            ..store.clone()
        };
        let error = query_claude_quota(
            &store,
            home.path(),
            &transport,
            &options(CredentialPreference::ClaudeCode),
        )
        .expect_err("expired");
        assert_eq!(
            error,
            ClaudeQuotaError::TokenExpired {
                source: CredentialSource::ClaudeCode
            }
        );
        assert!(!error.to_string().contains("fake-code-token"));
    }
    assert_eq!(transport.calls(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_decrypts_and_auto_falls_back() {
    let home = TempHome::new();
    let blob = include_str!("fixtures/claude_quota/desktop_blob.txt").trim();
    home.write_desktop(blob);
    let store = FakeStore {
        desktop: Some("fake-safe-storage-password".to_owned()),
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let transport = FakeTransport::usage();
    let desktop = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::Auto),
    )
    .expect("desktop quota");
    assert_eq!(desktop.source, CredentialSource::ClaudeDesktop);
    assert!(desktop.fallback_reason.is_none());
    let denied = FakeStore {
        denied: true,
        ..store.clone()
    };
    let error = query_claude_quota(
        &denied,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeDesktop),
    )
    .expect_err("denied");
    assert_eq!(
        error,
        ClaudeQuotaError::CredentialDenied {
            source: CredentialSource::ClaudeDesktop
        }
    );
    let fallback = query_claude_quota(
        &denied,
        home.path(),
        &transport,
        &options(CredentialPreference::Auto),
    )
    .expect("fallback");
    assert_eq!(fallback.source, CredentialSource::ClaudeCode);
    assert!(fallback
        .fallback_reason
        .as_deref()
        .is_some_and(|v| v.contains("拒绝")));
    let no_desktop = FakeStore {
        desktop: None,
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let fallback = query_claude_quota(
        &no_desktop,
        home.path(),
        &transport,
        &options(CredentialPreference::Auto),
    )
    .expect("fallback no key");
    assert_eq!(fallback.source, CredentialSource::ClaudeCode);
    assert!(fallback.fallback_reason.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn security_subprocess_handles_json_hex_exit_codes_and_timeout() {
    let home = TempHome::new();
    let transport = FakeTransport::usage();
    let script = home.security_script(&format!(
        "[ \"$1\" = find-generic-password ] && [ \"$2\" = -s ] && [ \"$3\" = 'Claude Code-credentials' ] && [ \"$4\" = -w ] && [ $# -eq 4 ] || exit 2\nprintf '%s\\n' '{CODE}'"
    ));
    let store = SystemSecretStore::with_security_path(script, Duration::from_secs(2));
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect("JSON credential");
    assert_eq!(result.source, CredentialSource::ClaudeCode);

    let hex: String = CODE
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let script = home.security_script(&format!("printf '%s\\n' '{hex}'"));
    let store = SystemSecretStore::with_security_path(script, Duration::from_secs(2));
    query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect("hex credential");

    let script = home.security_script("exit 44");
    let store = SystemSecretStore::with_security_path(script, Duration::from_secs(2));
    assert_eq!(
        store.generic_password("Claude Code-credentials", None),
        Ok(None)
    );
    let script = home.security_script("exit 12");
    let store = SystemSecretStore::with_security_path(script, Duration::from_secs(2));
    assert_eq!(
        store.generic_password("Claude Code-credentials", None),
        Err(SecretError::Unavailable)
    );
    let pid_file = home.path().join("security.pid");
    let script = home.security_script(&format!(
        "printf '%s' \"$$\" > '{}'\nexec sleep 3",
        pid_file.display()
    ));
    let store = SystemSecretStore::with_security_path(script, Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(
        store.generic_password("Claude Code-credentials", None),
        Err(SecretError::Unavailable)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid = fs::read_to_string(pid_file).expect("fake security started");
    assert!(!std::process::Command::new("/bin/kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("inspect fake security process")
        .success());
}

#[cfg(target_os = "macos")]
#[test]
fn security_subprocess_adds_account_only_when_supplied() {
    let home = TempHome::new();
    let script = home.security_script(
        "[ $# -eq 6 ] && [ \"$5\" = -a ] && [ \"$6\" = test-account ] || exit 2\nprintf 'test-secret\\n'",
    );
    let store = SystemSecretStore::with_security_path(script, Duration::from_secs(2));
    assert_eq!(
        store
            .generic_password("Claude Code-credentials", Some("test-account"))
            .expect("account lookup")
            .as_deref()
            .map(Vec::as_slice),
        Some(b"test-secret".as_slice())
    );
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_keychain_timeout_is_bounded_and_auto_falls_back() {
    #[derive(Clone)]
    struct BlockingStore;
    impl SecretStore for BlockingStore {
        fn generic_password(
            &self,
            service: &str,
            _: Option<&str>,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
            if service == "Claude Safe Storage" {
                std::thread::sleep(Duration::from_millis(200));
                return Ok(None);
            }
            Ok(Some(Zeroizing::new(CODE.as_bytes().to_vec())))
        }
    }
    let home = TempHome::new();
    home.write_desktop(include_str!("fixtures/claude_quota/desktop_blob.txt").trim());
    let transport = FakeTransport::usage();
    let mut desktop_options = options(CredentialPreference::ClaudeDesktop);
    desktop_options.keychain_timeout = Duration::from_millis(20);
    let started = Instant::now();
    let error = query_claude_quota(&BlockingStore, home.path(), &transport, &desktop_options)
        .expect_err("desktop keychain timeout");
    assert!(started.elapsed() < Duration::from_millis(150));
    assert_eq!(
        error,
        ClaudeQuotaError::KeychainTimeout {
            source: CredentialSource::ClaudeDesktop
        }
    );
    let mut auto_options = options(CredentialPreference::Auto);
    auto_options.keychain_timeout = Duration::from_millis(20);
    let result = query_claude_quota(&BlockingStore, home.path(), &transport, &auto_options)
        .expect("fallback to Claude Code");
    assert_eq!(result.source, CredentialSource::ClaudeCode);
    assert_eq!(
        result.fallback_reason.as_deref(),
        Some("等待 Claude 桌面版钥匙串授权超时")
    );
    assert_eq!(transport.calls(), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_legacy_cache_is_used_when_v2_cannot_be_decrypted() {
    let home = TempHome::new();
    let blob = include_str!("fixtures/claude_quota/desktop_blob.txt").trim();
    home.write_desktop(blob);
    fs::write(
        home.path()
            .join("Library/Application Support/Claude/config.json"),
        format!(r#"{{"oauth:tokenCacheV2":"!!!","oauth:tokenCache":"{blob}"}}"#),
    )
    .expect("write fake caches");
    let store = FakeStore {
        desktop: Some("fake-safe-storage-password".to_owned()),
        code: None,
        denied: false,
    };
    let transport = FakeTransport::usage();
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeDesktop),
    )
    .expect("legacy quota");
    assert_eq!(result.source, CredentialSource::ClaudeDesktop);
    assert_eq!(transport.calls(), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn auto_falls_back_from_expired_desktop_token() {
    let home = TempHome::new();
    let plaintext = br#"{"cache":{"token":"fake-expired-desktop-token","expiresAt":1000}}"#;
    let mut key = [0_u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"fake-safe-storage-password", b"saltysalt", 1003, &mut key);
    let mut buffer = vec![0_u8; plaintext.len() + 16];
    buffer[..plaintext.len()].copy_from_slice(plaintext);
    let encrypted = cbc::Encryptor::<Aes128>::new_from_slices(&key, b"                ")
        .expect("fake key and iv")
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, plaintext.len())
        .expect("fake encryption");
    let blob =
        base64::engine::general_purpose::STANDARD.encode([b"v10".as_slice(), encrypted].concat());
    home.write_desktop(&blob);
    let store = FakeStore {
        desktop: Some("fake-safe-storage-password".to_owned()),
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let transport = FakeTransport::usage();
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::Auto),
    )
    .expect("code fallback");
    assert_eq!(result.source, CredentialSource::ClaudeCode);
    assert!(result
        .fallback_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("已过期")));
    assert_eq!(transport.calls(), 1);
}

#[cfg(windows)]
#[test]
fn windows_desktop_is_unsupported_and_auto_uses_code_file() {
    let home = TempHome::new();
    home.write_code(CODE);
    let store = FakeStore {
        desktop: None,
        code: None,
        denied: false,
    };
    let transport = FakeTransport::usage();
    assert_eq!(
        query_claude_quota(
            &store,
            home.path(),
            &transport,
            &options(CredentialPreference::ClaudeDesktop)
        )
        .expect_err("unsupported"),
        ClaudeQuotaError::Unsupported
    );
    let result = query_claude_quota(
        &store,
        home.path(),
        &transport,
        &options(CredentialPreference::Auto),
    )
    .expect("auto code");
    assert_eq!(result.source, CredentialSource::ClaudeCode);
    assert!(result.fallback_reason.is_some());
}

#[test]
fn absent_or_malformed_code_credentials_do_not_call_transport() {
    let home = TempHome::new();
    let store = FakeStore {
        desktop: None,
        code: None,
        denied: false,
    };
    let transport = FakeTransport::usage();
    assert_eq!(
        query_claude_quota(
            &store,
            home.path(),
            &transport,
            &options(CredentialPreference::ClaudeCode)
        )
        .expect_err("absent"),
        ClaudeQuotaError::NoCredential
    );
    home.write_code("not json");
    #[cfg(target_os = "macos")]
    let store = FakeStore {
        code: Some("not json".to_owned()),
        ..store.clone()
    };
    assert_eq!(
        query_claude_quota(
            &store,
            home.path(),
            &transport,
            &options(CredentialPreference::ClaudeCode)
        )
        .expect_err("malformed"),
        ClaudeQuotaError::Parse
    );
    assert_eq!(transport.calls(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn code_keychain_denial_is_distinct_from_missing_credentials() {
    #[derive(Clone)]
    struct DeniedCodeStore;
    impl SecretStore for DeniedCodeStore {
        fn generic_password(
            &self,
            service: &str,
            _: Option<&str>,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
            assert_eq!(service, "Claude Code-credentials");
            Err(SecretError::Denied)
        }
    }
    let home = TempHome::new();
    let transport = FakeTransport::usage();
    let error = query_claude_quota(
        &DeniedCodeStore,
        home.path(),
        &transport,
        &options(CredentialPreference::ClaudeCode),
    )
    .expect_err("denied");
    assert_eq!(
        error,
        ClaudeQuotaError::CredentialDenied {
            source: CredentialSource::ClaudeCode
        }
    );
    assert_eq!(transport.calls(), 0);
}

#[test]
fn http_errors_never_expose_token_or_body() {
    let home = TempHome::new();
    home.write_code(CODE);
    let store = FakeStore {
        desktop: None,
        code: Some(CODE.to_owned()),
        denied: false,
    };
    let cases = [
        (
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"OAuth access token has expired. Re-authenticate to continue."}}"#,
            None,
            ClaudeQuotaError::Unauthorized {
                error_type: Some("authentication_error".into()),
            },
        ),
        (
            403,
            r#"{"error":{"type":"permission_error","message":"fake-code-token"}}"#,
            None,
            ClaudeQuotaError::Unauthorized {
                error_type: Some("permission_error".into()),
            },
        ),
        (
            429,
            "fake-code-token",
            Some("12"),
            ClaudeQuotaError::RateLimited {
                retry_after: Some(Duration::from_secs(12)),
            },
        ),
        (
            429,
            "",
            None,
            ClaudeQuotaError::RateLimited { retry_after: None },
        ),
        (
            500,
            "fake-code-token",
            None,
            ClaudeQuotaError::Http { status: 500 },
        ),
        (
            200,
            "not json fake-code-token",
            None,
            ClaudeQuotaError::Parse,
        ),
    ];
    for (status, body, retry_after, expected) in cases {
        let transport = FakeTransport::status(status, body, retry_after);
        let error = query_claude_quota(
            &store,
            home.path(),
            &transport,
            &options(CredentialPreference::ClaudeCode),
        )
        .expect_err("error");
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("fake-code-token"));
        assert!(!error.to_string().contains("Re-authenticate"));
    }
}

#[test]
fn proxy_parsers_cover_static_and_pac_settings() {
    let scutil = "<dictionary> {\n  HTTPSEnable : 1\n  HTTPSProxy : 127.0.0.1\n  HTTPSPort : 7890\n  HTTPEnable : 1\n  HTTPProxy : 127.0.0.2\n  HTTPPort : 8080\n}";
    assert_eq!(
        parse_scutil_proxy(scutil).as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        parse_scutil_proxy(&scutil.replace("HTTPSEnable : 1", "HTTPSEnable : 0")).as_deref(),
        Some("http://127.0.0.2:8080")
    );
    assert_eq!(
        parse_scutil_proxy(&format!("{scutil}\n ProxyAutoConfigEnable : 1")),
        None
    );
    assert_eq!(
        parse_windows_proxy(true, "proxy.local:7890").as_deref(),
        Some("http://proxy.local:7890")
    );
    assert_eq!(
        parse_windows_proxy(true, "http=other:80;https=secure:7890").as_deref(),
        Some("http://secure:7890")
    );
    assert_eq!(parse_windows_proxy(false, "proxy.local:7890"), None);
}

#[test]
fn every_error_display_omits_test_tokens() {
    let errors = [
        ClaudeQuotaError::NoCredential,
        ClaudeQuotaError::CredentialDenied {
            source: CredentialSource::ClaudeDesktop,
        },
        ClaudeQuotaError::KeychainTimeout {
            source: CredentialSource::ClaudeDesktop,
        },
        ClaudeQuotaError::TokenExpired {
            source: CredentialSource::ClaudeCode,
        },
        ClaudeQuotaError::Unauthorized {
            error_type: Some("authentication_error".into()),
        },
        ClaudeQuotaError::RateLimited {
            retry_after: Some(Duration::from_secs(5)),
        },
        ClaudeQuotaError::Http { status: 500 },
        ClaudeQuotaError::Network,
        ClaudeQuotaError::Parse,
        ClaudeQuotaError::Unsupported,
    ];
    for error in errors {
        let display = error.to_string();
        assert!(!display.contains("fake-code-token"));
        assert!(!display.contains("fake-access-token-AAA"));
        assert!(!display.contains("fake-refresh-token"));
    }
}

#[test]
fn memory_store_reads_the_keychain_once_until_forgotten() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use token_audit_core::claude_quota::MemorySecretStore;

    #[derive(Clone, Default)]
    struct CountingStore(Arc<AtomicUsize>);
    impl SecretStore for CountingStore {
        fn generic_password(
            &self,
            service: &str,
            _account: Option<&str>,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok((service != "missing").then(|| Zeroizing::new(b"secret".to_vec())))
        }
    }

    let inner = CountingStore::default();
    let store = MemorySecretStore::new(inner.clone());
    for _ in 0..3 {
        let secret = store
            .generic_password("Claude Code-credentials", None)
            .expect("read");
        assert_eq!(secret.as_deref().map(Vec::as_slice), Some(&b"secret"[..]));
    }
    assert_eq!(inner.0.load(Ordering::SeqCst), 1);

    // A missing item is not remembered, so it is looked up again next time.
    assert!(store
        .generic_password("missing", None)
        .expect("read")
        .is_none());
    assert!(store
        .generic_password("missing", None)
        .expect("read")
        .is_none());
    assert_eq!(inner.0.load(Ordering::SeqCst), 3);

    store.forget();
    store
        .generic_password("Claude Code-credentials", None)
        .expect("read");
    assert_eq!(inner.0.load(Ordering::SeqCst), 4);
}

#[test]
fn silent_memory_store_never_asks_interactively() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use token_audit_core::claude_quota::MemorySecretStore;

    #[derive(Clone, Default)]
    struct Store {
        prompts: Arc<AtomicUsize>,
        silent_reads: Arc<AtomicUsize>,
    }
    impl SecretStore for Store {
        fn generic_password(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            Ok(Some(Zeroizing::new(b"approved".to_vec())))
        }
        fn generic_password_silently(
            &self,
            service: &str,
            _: Option<&str>,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
            self.silent_reads.fetch_add(1, Ordering::SeqCst);
            Ok((service == "always-allowed").then(|| Zeroizing::new(b"silent".to_vec())))
        }
    }

    let inner = Store::default();
    let store = MemorySecretStore::new(inner.clone());
    let quiet = store.silent();

    // Needs approval: absent, and no dialog.
    assert!(quiet
        .generic_password("Claude Safe Storage", Some("Claude"))
        .expect("read")
        .is_none());
    // Already allowed: read without a dialog and remembered for both views.
    assert_eq!(
        quiet
            .generic_password("always-allowed", None)
            .expect("read")
            .as_deref()
            .map(Vec::as_slice),
        Some(&b"silent"[..])
    );
    store
        .generic_password("always-allowed", None)
        .expect("read");
    assert_eq!(inner.prompts.load(Ordering::SeqCst), 0);

    // A user action may prompt; afterwards background reads reuse it silently.
    store
        .generic_password("Claude Safe Storage", Some("Claude"))
        .expect("read");
    let reused = quiet
        .generic_password("Claude Safe Storage", Some("Claude"))
        .expect("read");
    assert_eq!(reused.as_deref().map(Vec::as_slice), Some(&b"approved"[..]));
    assert_eq!(inner.prompts.load(Ordering::SeqCst), 1);
    assert_eq!(inner.silent_reads.load(Ordering::SeqCst), 2);
}
