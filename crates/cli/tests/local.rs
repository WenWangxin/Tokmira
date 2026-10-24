use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-local-cli-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temporary home");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temporary home");
    }
}

#[test]
fn local_commands_use_only_fixture_home_and_saved_quota() {
    let temp = Temp::new();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/m7a/claude.jsonl");
    let log = temp.path().join(".claude/projects/fixture/session-a.jsonl");
    fs::create_dir_all(log.parent().expect("log parent")).expect("log directory");
    fs::copy(fixture, log).expect("handwritten log");
    let quota = temp.path().join("data/quota/claude.jsonl");
    fs::create_dir_all(quota.parent().expect("quota parent")).expect("quota directory");
    fs::write(quota, b"{\"at\":\"2026-10-24T10:06:00Z\",\"subscription_type\":\"pro\",\"five_hour\":{\"used_percent\":1.0,\"resets_at\":\"2026-10-24T15:00:00Z\",\"window_minutes\":300},\"seven_day\":null,\"weekly_breakdown\":null}\n").expect("handwritten snapshot");
    for command in ["attribution", "overview"] {
        let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
            .arg(command)
            .arg("--home")
            .arg(temp.path())
            .arg("--data-dir")
            .arg(temp.path().join("data"))
            .arg("--json")
            .output()
            .expect("run local command");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON result");
        if command == "attribution" {
            assert_eq!(value["windows"][0]["attributed_total"], 1.0);
            assert_eq!(value["top_sessions"][0]["session_id_prefix"], "session-");
        } else {
            assert_eq!(value["tools"][0]["plan"], "pro");
            assert_eq!(value["tools"][0]["subscription_usd"], 20.0);
            assert_eq!(value["subscription_total_usd"], 20.0);
            assert!(value["subscription_value_multiple"].is_number());
            assert!(value["today_token_total"].is_number());
            assert_eq!(
                value["daily_cache_hit_rate"].as_array().map(Vec::len),
                Some(14)
            );
            assert!(value["trends"][0]["today"].is_array());
            assert!(value["trends"][0]["week"].is_array());
            assert!(value["recent_high_prompts"].is_array());
            let now = Utc::now();
            let snapshot_at = DateTime::parse_from_rfc3339("2026-10-24T10:06:00Z")
                .expect("fixture timestamp")
                .with_timezone(&Utc);
            let reset_at = DateTime::parse_from_rfc3339("2026-10-24T15:00:00Z")
                .expect("fixture reset")
                .with_timezone(&Utc);
            let five_hour = &value["tools"][0]["five_hour"];
            assert_eq!(five_hour["window_passed"], now >= reset_at);
            if now < snapshot_at || now >= reset_at {
                assert!(five_hour["used_percent"].is_null());
            } else {
                assert_eq!(five_hour["used_percent"], 1.0);
            }
        }
    }
}

#[test]
fn usage_json_uses_only_fixture_directories() {
    let temp = Temp::new();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/tests/fixtures/usage_cli_snapshot.jsonl");
    let quota = temp.path().join("data/quota/claude.jsonl");
    fs::create_dir_all(quota.parent().expect("quota parent")).expect("quota directory");
    fs::copy(fixture, quota).expect("handwritten quota fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
        .args(["usage", "--tool", "claude", "--home"])
        .arg(temp.path())
        .arg("--data-dir")
        .arg(temp.path().join("data"))
        .arg("--window")
        .arg("2020-01-01T05:00:00Z")
        .arg("--json")
        .output()
        .expect("run usage command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("usage JSON");
    assert_eq!(value["windows"][0]["window_id"], "2020-01-01T05:00:00Z");
    assert_eq!(value["detail"]["used_percent"], 12.0);
    assert_eq!(value["detail"]["unattributed_total"], 12.0);
    assert_eq!(
        value["detail"]["sessions"].as_array().map(Vec::len),
        Some(0)
    );
}
