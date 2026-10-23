use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use token_audit_core::claude::scan_claude;
use token_audit_core::codex::scan_codex;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "token-audit-scan-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create fixture home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove fixture home");
    }
}

#[test]
fn scan_json_combines_both_scanners_and_reports_unpriced_events() {
    let home = TempHome::new();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures");
    let claude_source = fixtures.join("claude/home/.claude/projects/demo-a/first.jsonl");
    let claude_dest = home.path().join(".claude/projects/demo-a");
    fs::create_dir_all(&claude_dest).expect("Claude fixture directory");
    fs::copy(claude_source, claude_dest.join("first.jsonl")).expect("Claude fixture");
    let codex_source = fixtures.join("codex/new.jsonl");
    let codex_dest = home.path().join(".codex/sessions/2026/10/14");
    fs::create_dir_all(&codex_dest).expect("Codex fixture directory");
    fs::copy(codex_source, codex_dest.join("rollout-a.jsonl")).expect("Codex fixture");

    let expected = scan_claude(home.path()).expect("Claude scan").events.len()
        + scan_codex(home.path()).expect("Codex scan").events.len();
    let output = Command::new(env!("CARGO_BIN_EXE_token-audit"))
        .args(["scan", "--home"])
        .arg(home.path())
        .args(["--pricing"])
        .arg(fixtures.join("pricing/empty.toml"))
        .arg("--json")
        .output()
        .expect("run CLI");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(json["total"]["events"], expected);
    assert_eq!(json["pricing_version"], "fixture");
    assert_eq!(json["total"]["unpriced_events"], expected);
    assert!(json["by_tool"].as_array().expect("tool groups").len() >= 2);
}
