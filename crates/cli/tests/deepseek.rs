use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-cli-deepseek-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("sessions/workspace/session-one")).expect("fixture home");
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
#[test]
fn scan_and_debug_deepseek_fixture() {
    let temp = Temp::new();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/deepseek/main.zstd");
    fs::copy(
        fixture,
        temp.path()
            .join("sessions/workspace/session-one/session.v4.jsonl.zstd"),
    )
    .expect("fixture");
    let bin = env!("CARGO_BIN_EXE_token-audit");
    let scan = Command::new(bin)
        .args(["scan", "--tool", "deepseek", "--json", "--home"])
        .arg(temp.path())
        .env("DSH_HOME", temp.path())
        .output()
        .expect("scan CLI");
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&scan.stdout).expect("scan JSON");
    assert_eq!(json["total"]["events"], 2);
    // Yuan prices apply; nothing is added to the dollar total.
    assert_eq!(json["total"]["unpriced_events"], 0);
    assert_eq!(json["total"]["estimated_cost_usd"], 0.0);
    assert!(json["total"]["estimated_cost_cny"].as_f64().expect("yuan") > 0.0);
    assert_eq!(json["by_tool"][0]["key"], "DeepSeek");
    assert_eq!(json["by_source"][0]["key"], "DeepSeek");
    let debug = Command::new(bin)
        .args(["debug", "deepseek", "--json", "--home"])
        .arg(temp.path())
        .env("DSH_HOME", temp.path())
        .output()
        .expect("debug CLI");
    assert!(
        debug.status.success(),
        "{}",
        String::from_utf8_lossy(&debug.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&debug.stdout).expect("debug JSON");
    assert_eq!(json["events_total"], 2);
    assert_eq!(json["stats"]["title_requests"], 1);
}
