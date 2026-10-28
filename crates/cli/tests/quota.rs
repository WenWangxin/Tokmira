#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn quota_codex_prints_official_data_from_fake_app_server() {
        let directory = std::env::temp_dir().join(format!(
            "token-audit-cli-quota-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).expect("temporary directory");
        let codex = directory.join("codex");
        fs::write(
            &codex,
            r#"#!/bin/sh
read -r first
printf '%s\n' '{"id":1,"result":{}}'
read -r second
read -r third
printf '%s\n' '{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","planType":"plus","primary":{"usedPercent":30,"windowDurationMins":300,"resetsAt":1792788614},"secondary":{"usedPercent":9,"windowDurationMins":10080,"resetsAt":1793352394}},"rateLimitResetCredits":{"availableCount":1,"credits":[]}}}'
read -r fourth
printf '%s\n' '{"id":3,"result":{"summary":{"lifetimeTokens":1000},"dailyUsageBuckets":[]}}'
"#,
        )
        .expect("fake codex");
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).expect("chmod fake codex");
        let executable = env!("CARGO_BIN_EXE_token-audit");
        let text = Command::new(executable)
            .args(["quota", "codex", "--codex"])
            .arg(&codex)
            .output()
            .expect("run quota text");
        assert!(
            text.status.success(),
            "{}",
            String::from_utf8_lossy(&text.stderr)
        );
        let text = String::from_utf8(text.stdout).expect("utf8 text");
        assert!(text.contains("套餐"));
        assert!(text.contains("30.0%"));
        assert!(text.contains("每周已用"));
        assert!(text.contains("Tokmira 不读取你的令牌"));

        let json = Command::new(executable)
            .args(["quota", "codex", "--codex"])
            .arg(&codex)
            .arg("--json")
            .output()
            .expect("run quota json");
        assert!(
            json.status.success(),
            "{}",
            String::from_utf8_lossy(&json.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&json.stdout).expect("json output");
        assert_eq!(json["plan_type"], "plus");
        assert_eq!(json["primary"]["used_percent"].as_f64(), Some(30.0));
        assert!(!json.to_string().contains("accountId"));

        let missing = Command::new(executable)
            .args(["quota", "codex", "--codex"])
            .arg(directory.join("missing"))
            .output()
            .expect("run missing codex");
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("不存在"));
        fs::remove_dir_all(&directory).expect("remove temporary directory");
    }
}
