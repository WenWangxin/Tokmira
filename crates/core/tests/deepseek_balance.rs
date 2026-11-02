use std::cell::RefCell;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use token_audit_core::deepseek_balance::{
    query_deepseek_balance, read_account_token, read_api_key, BalanceError, BalanceOptions,
    BalanceResponse, BalanceTransport, ClientIdentity, DeepSeekCredential, DeepSeekRoute,
    ACCOUNT_URL, API_KEY_URL,
};
use token_audit_core::deepseek_view::BalanceOrigin;
use zeroize::Zeroizing;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-deepseek-balance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary directory");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const ACCOUNT: &str = "version: 1
records:
  client-connection/browser-session:
    kind: grant
    payload:
      version: 1
      secret: not-a-token
  deepseek-account-platform/default:
    kind: grant
    payload:
      version: 1
      token: sample-account-token
      issuer: https://platform.deepseek.com
";

type Call = (String, Vec<(String, String)>);
struct Fake {
    status: u16,
    body: &'static str,
    calls: RefCell<Vec<Call>>,
}
impl Fake {
    fn new(status: u16, body: &'static str) -> Self {
        Self {
            status,
            body,
            calls: RefCell::new(Vec::new()),
        }
    }
}
impl BalanceTransport for Fake {
    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _proxy: Option<&str>,
        _timeout: Duration,
    ) -> Result<BalanceResponse, BalanceError> {
        self.calls.borrow_mut().push((
            url.to_owned(),
            headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        ));
        Ok(BalanceResponse {
            status: self.status,
            body: Zeroizing::new(self.body.as_bytes().to_vec()),
        })
    }
}

fn options(credential: DeepSeekCredential) -> BalanceOptions {
    BalanceOptions {
        credential,
        proxy: None,
        timeout: Duration::from_secs(5),
        client: ClientIdentity {
            windows: false,
            version: "1.3.5".into(),
            chinese: true,
            utc_offset_seconds: 28800,
        },
    }
}

#[test]
fn account_route_reads_both_wallets_and_leaves_credentials_alone() {
    let temp = Temp::new();
    let file = temp.path().join(".credentials.yaml");
    fs::write(&file, ACCOUNT).expect("credentials");
    let transport = Fake::new(
        200,
        r#"{"code":0,"msg":"","data":{"biz_code":0,"biz_msg":"","biz_data":{
            "normal_wallets":[{"currency":"CNY","balance":"76.40"}],
            "bonus_wallets":[{"currency":"CNY","balance":"10"},{"currency":"USD","balance":"3"}]}}}"#,
    );
    let now = Utc.with_ymd_and_hms(2026, 11, 2, 1, 0, 0).unwrap();
    let balance = query_deepseek_balance(
        temp.path(),
        None,
        &transport,
        &options(DeepSeekCredential::Auto),
        now,
    )
    .expect("balance");
    assert_eq!(balance.origin, BalanceOrigin::Official);
    assert_eq!(balance.topped_up, Some(76.4));
    assert_eq!(balance.granted, Some(10.0));
    assert!((balance.total - 86.4).abs() < 1e-9);
    let calls = transport.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, ACCOUNT_URL);
    let header = |name: &str| {
        calls[0]
            .1
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(
        header("x-dsh-auth-token").as_deref(),
        Some("sample-account-token")
    );
    assert_eq!(header("x-client-platform").as_deref(), Some("desktop-mac"));
    assert_eq!(header("x-client-locale").as_deref(), Some("zh_CN"));
    assert_eq!(header("x-client-timezone-offset").as_deref(), Some("28800"));
    assert_eq!(fs::read_to_string(&file).expect("unchanged"), ACCOUNT);
}

#[test]
fn rejected_logins_report_without_touching_files() {
    let temp = Temp::new();
    let file = temp.path().join(".credentials.yaml");
    fs::write(&file, ACCOUNT).expect("credentials");
    let now = Utc::now();
    for transport in [
        Fake::new(401, ""),
        Fake::new(200, r#"{"code":40003,"msg":"expired"}"#),
    ] {
        let error = query_deepseek_balance(
            temp.path(),
            None,
            &transport,
            &options(DeepSeekCredential::Account),
            now,
        )
        .expect_err("rejected");
        assert_eq!(error, BalanceError::Unauthorized(DeepSeekRoute::Account));
    }
    assert_eq!(fs::read_to_string(&file).expect("unchanged"), ACCOUNT);
    assert_eq!(
        query_deepseek_balance(
            temp.path(),
            None,
            &Fake::new(500, ""),
            &options(DeepSeekCredential::Account),
            now,
        ),
        Err(BalanceError::Network)
    );
}

#[test]
fn api_key_route_and_its_lookup_order() {
    let temp = Temp::new();
    // No account login: Auto falls through to the API key from the environment.
    assert!(read_account_token(temp.path()).is_none());
    assert_eq!(
        read_api_key(temp.path(), Some(OsStr::new(" sk-env ")))
            .as_deref()
            .map(String::as_str),
        Some("sk-env")
    );
    fs::write(
        temp.path().join(".env"),
        "# note\nexport DEEPSEEK_API_KEY=\"sk-dotenv\"\n",
    )
    .expect("env");
    assert_eq!(
        read_api_key(temp.path(), None)
            .as_deref()
            .map(String::as_str),
        Some("sk-dotenv")
    );
    fs::write(
        temp.path().join(".credentials.yaml"),
        "version: 1\nrefs:\n  DEEPSEEK_API_KEY: 'sk-stored'\n",
    )
    .expect("credentials");
    assert_eq!(
        read_api_key(temp.path(), Some(OsStr::new("sk-env")))
            .as_deref()
            .map(String::as_str),
        Some("sk-stored")
    );
    let transport = Fake::new(
        200,
        r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"5.60","granted_balance":"0.00","topped_up_balance":"5.60"}]}"#,
    );
    let balance = query_deepseek_balance(
        temp.path(),
        None,
        &transport,
        &options(DeepSeekCredential::Auto),
        Utc::now(),
    )
    .expect("balance");
    assert!((balance.total - 5.6).abs() < 1e-9);
    let calls = transport.calls.borrow();
    assert_eq!(calls[0].0, API_KEY_URL);
    assert!(calls[0]
        .1
        .iter()
        .any(|(name, value)| name == "Authorization" && value == "Bearer sk-stored"));
}

#[test]
fn foreign_issuers_other_currencies_and_missing_logins() {
    let temp = Temp::new();
    fs::write(
        temp.path().join(".credentials.yaml"),
        ACCOUNT.replace("https://platform.deepseek.com", "https://example.test"),
    )
    .expect("credentials");
    assert!(read_account_token(temp.path()).is_none());
    assert_eq!(
        query_deepseek_balance(
            temp.path(),
            None,
            &Fake::new(200, "{}"),
            &options(DeepSeekCredential::Auto),
            Utc::now(),
        ),
        Err(BalanceError::NoCredential)
    );
    fs::write(temp.path().join(".credentials.yaml"), ACCOUNT).expect("credentials");
    let usd = Fake::new(
        200,
        r#"{"code":0,"data":{"biz_code":0,"biz_data":{"normal_wallets":[{"currency":"USD","balance":"2"}],"bonus_wallets":[]}}}"#,
    );
    assert_eq!(
        query_deepseek_balance(
            temp.path(),
            None,
            &usd,
            &options(DeepSeekCredential::Account),
            Utc::now(),
        ),
        Err(BalanceError::Currency)
    );
}
