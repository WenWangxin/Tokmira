use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use token_audit_core::claude::scan_claude;
use token_audit_core::codex::scan_codex;
use token_audit_core::environment::scan_environment;
use token_audit_core::pricing::Pricing;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-m12-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temp home");
        Self(path)
    }
    fn put(&self, path: &str, bytes: &[u8]) {
        let destination = self.0.join(path);
        fs::create_dir_all(destination.parent().expect("parent")).expect("mkdir");
        fs::write(destination, bytes).expect("write fixture");
    }
    fn fixture(&self, source: &str, destination: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m12")
            .join(source);
        self.put(destination, &fs::read(path).expect("fixture"));
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn claude_usage_metadata_and_compaction_are_selected() {
    let home = Home::new();
    home.fixture("claude.jsonl", ".claude/projects/fake/session.jsonl");
    let scan = scan_claude(&home.0).expect("scan");
    assert_eq!(scan.events.len(), 1);
    let event = &scan.events[0];
    assert!(event.fast);
    assert_eq!(event.effort.as_deref(), Some("xhigh"));
    assert_eq!(event.web_search_requests, 3);
    assert_eq!(event.tokens.output, 20);
    assert_eq!(scan.compactions.len(), 1);
    assert_eq!(scan.compactions[0].trigger.as_deref(), Some("manual"));
    assert_eq!(scan.compactions[0].pre_tokens, Some(90_000));
    assert_eq!(scan.compactions[0].post_tokens, Some(10_000));
    assert!(!format!("{scan:?}").contains("[redacted]"));
}

#[test]
fn codex_effort_tracks_turns_and_output_includes_reasoning() {
    let home = Home::new();
    home.fixture(
        "codex.jsonl",
        ".codex/sessions/2026/10/20/rollout-m12.jsonl",
    );
    let scan = scan_codex(&home.0).expect("scan");
    assert_eq!(scan.events.len(), 2);
    assert_eq!(scan.events[0].effort.as_deref(), Some("xhigh"));
    assert_eq!(scan.events[0].model, "gpt-6-astra");
    assert_eq!(scan.events[0].tokens.output, 100);
    assert_eq!(scan.events[0].tokens.reasoning, 80);
    assert_eq!(
        scan.events[0].tokens.output - scan.events[0].tokens.reasoning,
        20
    );
    assert_eq!(scan.events[1].effort.as_deref(), Some("low"));
    assert_eq!(scan.stats.reasoning_included_samples, 1);
    assert_eq!(scan.stats.reasoning_separate_samples, 0);
    assert_eq!(scan.compactions.len(), 1);
    assert_eq!(scan.compactions[0].pre_tokens, Some(90_000));
    let price = Pricing::bundled().expect("prices");
    let cost = price
        .cost_usd(&scan.events[0].model, &scan.events[0].tokens)
        .expect("known model");
    let expected = (400.0 * 10.0 + 600.0 * 1.0 + 100.0 * 50.0) / 1_000_000.0;
    assert!((cost - expected).abs() < 0.000_000_1);
    assert!(!format!("{scan:?}").contains("[redacted]"));
}

#[test]
fn environment_reads_names_counts_and_instruction_sizes() {
    let home = Home::new();
    let project = home.0.join("project");
    home.put(".claude.json", format!(r#"{{"mcpServers":{{"global":{{"command":"[redacted]"}}}},"projects":{{"{}":{{"mcpServers":{{"project":{{}},"disabled":{{}}}},"disabledMcpServers":["disabled"]}}}}}}"#, project.display()).as_bytes());
    home.put(
        ".claude/settings.json",
        br#"{"enabledPlugins":{"one":true,"two":false,"three":true}}"#,
    );
    home.put(
        "project/.mcp.json",
        br#"{"mcpServers":{"local":{},"disabled":{}}}"#,
    );
    home.put(".codex/config.toml", b"[mcp_servers.first]\ncommand = '[redacted]'\n[mcp_servers.first.env]\nKEY = '[redacted]'\n[mcp_servers.\"quoted.name\"]\nenabled = true\n[mcp_servers.second]\nenabled = false\n");
    home.put(".claude/CLAUDE.md", b"123456");
    home.put("project/CLAUDE.md", b"123");
    home.put(".codex/AGENTS.md", b"1234");
    home.put("project/AGENTS.md", b"12");
    let unrelated = home.0.join("unrelated");
    home.put("unrelated/.mcp.json", br#"{"mcpServers":{"foreign":{}}}"#);
    let snapshot = scan_environment(&home.0, &[project.clone(), unrelated.clone()]);
    assert_eq!(snapshot.claude_global_mcp_names, ["global"]);
    assert_eq!(
        snapshot.claude_project_mcp_names[&project],
        ["local", "project"]
    );
    assert_eq!(snapshot.claude_project_mcp_names[&unrelated], ["foreign"]);
    assert_eq!(snapshot.codex_global_mcp_names, ["first", "quoted.name"]);
    assert_eq!(snapshot.claude_plugins, 2);
    assert_eq!(snapshot.claude_instruction_bytes, 6);
    assert_eq!(snapshot.codex_instruction_bytes, 4);
    assert_eq!(
        snapshot.claude_project_instruction_bytes[&home.0.join("project")],
        3
    );
    assert_eq!(
        snapshot.codex_project_instruction_bytes[&home.0.join("project")],
        2
    );
}
