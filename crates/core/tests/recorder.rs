use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use token_audit_core::recorder::{
    clear_records, executable_command, install, normalize_executable_path, plan_install,
    plan_uninstall, read_hook_events, read_statusline_records, record_hook_event,
    record_statusline, status, uninstall, RecorderParts,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-recorder-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create temp dir");
        Self(path)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn data(&self) -> PathBuf {
        self.0.join("data")
    }
    fn settings(&self) -> PathBuf {
        self.home().join(".claude/settings.json")
    }
    fn exe(&self) -> PathBuf {
        self.0.join("token-audit")
    }
    fn put_settings(&self, text: &str) {
        fs::create_dir_all(self.home().join(".claude")).expect("create settings dir");
        fs::write(self.settings(), text).expect("settings");
    }
    fn read_settings(&self) -> Value {
        serde_json::from_slice(&fs::read(self.settings()).expect("settings bytes")).expect("json")
    }
    fn backups(&self) -> usize {
        fs::read_dir(self.data().join("recorder/backups"))
            .expect("backups")
            .count()
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temp dir");
    }
}
fn both() -> RecorderParts {
    RecorderParts {
        statusline: true,
        hooks: true,
    }
}

#[test]
fn installs_and_restores_existing_configuration_with_order_and_backups() {
    let temp = TempDir::new();
    let original = r#"{"alpha":1,"statusLine":{"type":"command","command":"printf old","padding":0},"hooks":{"Stop":[{"matcher":"x","hooks":[{"type":"command","command":"printf stop"}]}],"SessionStart":[]},"omega":2}"#;
    temp.put_settings(original);
    let original_value: Value = serde_json::from_str(original).expect("json");
    let preview = plan_install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("plan");
    assert_eq!(preview.changes.len(), 5);
    assert_eq!(temp.backups_if_exists(), 0);
    let result = install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("install");
    assert_eq!(result.changes.len(), preview.changes.len());
    assert_eq!(temp.backups(), 1);
    assert_eq!(
        fs::read(result.backup_path.expect("backup")).expect("backup bytes"),
        original.as_bytes()
    );
    let settings = temp.read_settings();
    assert_eq!(
        settings
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["alpha", "statusLine", "hooks", "omega"]
    );
    assert_eq!(settings["statusLine"]["padding"], 0);
    assert_eq!(
        settings["hooks"]["Stop"][0],
        original_value["hooks"]["Stop"][0]
    );
    assert_eq!(
        settings["hooks"]["Stop"].as_array().expect("array").len(),
        2
    );
    assert_eq!(
        settings["hooks"]["SessionStart"]
            .as_array()
            .expect("array")
            .len(),
        1
    );
    let second = install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("repeat install");
    assert!(second.changes.is_empty());
    assert_eq!(temp.backups(), 1);
    let unplan = plan_uninstall(&temp.home(), &temp.data()).expect("uninstall plan");
    assert_eq!(unplan.changes.len(), 5);
    uninstall(&temp.home(), &temp.data()).expect("uninstall");
    assert_eq!(temp.backups(), 2);
    assert_eq!(temp.read_settings().to_string(), original_value.to_string());
}
impl TempDir {
    fn backups_if_exists(&self) -> usize {
        fs::read_dir(self.data().join("recorder/backups"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }
}

#[test]
fn new_settings_and_empty_hooks_restore_cleanly() {
    for original in [
        None,
        Some("{}"),
        Some("{\"hooks\":{}}"),
        Some("{\"hooks\":{\"Stop\":[]}}"),
    ] {
        let temp = TempDir::new();
        if let Some(original) = original {
            temp.put_settings(original);
        }
        install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("install");
        uninstall(&temp.home(), &temp.data()).expect("uninstall");
        if let Some(original) = original {
            let expected: Value = serde_json::from_str(original).expect("expected");
            assert_eq!(temp.read_settings().to_string(), expected.to_string());
        } else {
            assert!(!temp.settings().exists());
        }
    }
}

#[test]
fn modified_statusline_is_left_alone_and_state_is_cleared() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"printf before"}}"#);
    install(
        &temp.home(),
        &temp.data(),
        &temp.exe(),
        RecorderParts {
            statusline: true,
            hooks: false,
        },
    )
    .expect("install");
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"printf user"}}"#);
    let result = uninstall(&temp.home(), &temp.data()).expect("uninstall");
    assert!(result.changes.is_empty());
    assert!(result.notes.iter().any(|note| note.contains("用户修改")));
    assert_eq!(temp.read_settings()["statusLine"]["command"], "printf user");
    assert!(!status(&temp.home(), &temp.data()).statusline_modified);
}

#[test]
fn invalid_settings_are_never_overwritten() {
    for original in ["{bad", "[]"] {
        let temp = TempDir::new();
        temp.put_settings(original);
        assert!(install(&temp.home(), &temp.data(), &temp.exe(), both()).is_err());
        assert_eq!(fs::read_to_string(temp.settings()).expect("read"), original);
        assert_eq!(temp.backups_if_exists(), 0);
    }
}

#[test]
fn records_whitelisted_fields_deduplicates_and_reads_bad_tail() {
    let temp = TempDir::new();
    let input = include_bytes!("fixtures/recorder/statusline.json");
    record_statusline(&temp.data(), input);
    record_statusline(&temp.data(), input);
    let (records, _) = read_statusline_records(&temp.data());
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]
            .five_hour
            .as_ref()
            .and_then(|w| w.used_percentage),
        Some(28.0)
    );
    assert_eq!(
        records[0]
            .prompt_cache
            .as_ref()
            .and_then(|c| c.recache_tokens_if_cold),
        Some(45315)
    );
    let raw = fs::read_to_string(temp.data().join("recorder/statusline.jsonl")).expect("read");
    assert!(
        !raw.contains("PRIVATE_") && !raw.contains("prompt\":") && !raw.contains("transcript_path")
    );
    record_statusline(&temp.data(), br#"{"session_id":"x","rate_limits":{}}"#);
    assert_eq!(read_statusline_records(&temp.data()).0.len(), 1);
    let mut raw = raw;
    raw.push_str("{\"incomplete\":");
    fs::write(temp.data().join("recorder/statusline.jsonl"), raw).expect("write tail");
    let (_, stats) = read_statusline_records(&temp.data());
    assert_eq!(stats.malformed_lines, 1);
    let changed = br#"{"session_id":"session-demo","rate_limits":{"five_hour":{"used_percentage":29,"resets_at":1792795300}}}"#;
    record_statusline(&temp.data(), changed);
    let (after_partial, stats) = read_statusline_records(&temp.data());
    assert_eq!(after_partial.len(), 2);
    assert_eq!(stats.malformed_lines, 1);
    record_hook_event(&temp.data(), include_bytes!("fixtures/recorder/event.json"));
    record_hook_event(
        &temp.data(),
        br#"{"hook_event_name":"Unknown","session_id":"x"}"#,
    );
    let (events, _) = read_hook_events(&temp.data());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "UserPromptSubmit");
    assert!(
        !fs::read_to_string(temp.data().join("recorder/hooks.jsonl"))
            .expect("hooks")
            .contains("PRIVATE_")
    );
    clear_records(&temp.data()).expect("clear");
    assert!(read_statusline_records(&temp.data()).0.is_empty());
    assert!(read_hook_events(&temp.data()).0.is_empty());
}

#[test]
fn records_valid_quota_and_event_when_other_fields_have_unexpected_types() {
    let temp = TempDir::new();
    record_statusline(
        &temp.data(),
        include_bytes!("fixtures/recorder/statusline_mixed_types.json"),
    );
    let (records, _) = read_statusline_records(&temp.data());
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]
            .five_hour
            .as_ref()
            .and_then(|w| w.used_percentage),
        Some(28.0)
    );
    assert!(records[0].seven_day.is_none());
    assert!(records[0].session_id.is_none());
    assert!(records[0].model.is_none());
    let cache = records[0].prompt_cache.as_ref().expect("cache");
    assert!(cache.ttl.is_none() && cache.hit_ratio.is_none() && cache.misses.is_none());
    assert_eq!(cache.recache_tokens_if_cold, Some(45315));
    record_statusline(
        &temp.data(),
        include_bytes!("fixtures/recorder/statusline_seven_day_valid.json"),
    );
    let (records, _) = read_statusline_records(&temp.data());
    assert_eq!(records.len(), 2);
    assert!(records[1].five_hour.is_none());
    assert_eq!(
        records[1]
            .seven_day
            .as_ref()
            .and_then(|w| w.used_percentage),
        Some(6.0)
    );
    record_hook_event(
        &temp.data(),
        include_bytes!("fixtures/recorder/event_mixed_types.json"),
    );
    let (events, _) = read_hook_events(&temp.data());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "UserPromptSubmit");
    assert!(events[0].session_id.is_none());
    let raw = fs::read_to_string(temp.data().join("recorder/statusline.jsonl")).expect("read");
    assert!(!raw.contains("PRIVATE_"));
}

#[test]
fn changed_executable_path_updates_commands_and_preserves_original_statusline() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"printf original"}}"#);
    let a = temp.0.join("A/token-audit");
    let b = temp.0.join("B/token-audit");
    install(&temp.home(), &temp.data(), &a, both()).expect("install A");
    let updated = install(&temp.home(), &temp.data(), &b, both()).expect("install B");
    assert_eq!(updated.changes.len(), 5);
    assert!(updated.changes.iter().all(|change| change.action == "更新"));
    let settings = temp.read_settings();
    assert!(settings["statusLine"]["command"]
        .as_str()
        .expect("command")
        .contains("/B/token-audit"));
    for event in ["SessionStart", "UserPromptSubmit", "Stop", "SubagentStop"] {
        let entries = settings["hooks"][event].as_array().expect("entries");
        assert_eq!(entries.len(), 1);
        assert!(entries[0]["hooks"][0]["command"]
            .as_str()
            .expect("command")
            .contains("/B/token-audit"));
    }
    uninstall(&temp.home(), &temp.data()).expect("uninstall");
    assert_eq!(
        temp.read_settings()["statusLine"]["command"],
        "printf original"
    );
    assert!(temp.read_settings().get("hooks").is_none());
}

#[test]
fn stale_state_command_still_removes_old_executable_entries() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"printf original"}}"#);
    let a = temp.0.join("A/token-audit");
    let b = temp.0.join("B/token-audit");
    install(&temp.home(), &temp.data(), &a, both()).expect("install A");
    let state_path = temp.data().join("recorder/state.json");
    let mut state: Value =
        serde_json::from_slice(&fs::read(&state_path).expect("state")).expect("json");
    state["statusline_command"] =
        Value::String(executable_command(&b, "statusline").expect("B command"));
    state["hooks_command"] =
        Value::String(executable_command(&b, "event --tool claude").expect("B command"));
    fs::write(&state_path, serde_json::to_vec(&state).expect("serialize")).expect("write state");
    let plan = uninstall(&temp.home(), &temp.data()).expect("uninstall B");
    assert_eq!(plan.changes.len(), 5);
    assert_eq!(
        temp.read_settings()["statusLine"]["command"],
        "printf original"
    );
    assert!(temp.read_settings().get("hooks").is_none());
}

#[test]
fn install_does_not_save_our_existing_command_as_original() {
    let temp = TempDir::new();
    let old = temp.0.join("A/token-audit");
    let new = temp.0.join("B/token-audit");
    let old_command = executable_command(&old, "statusline").expect("old command");
    temp.put_settings(
        &serde_json::json!({"statusLine":{"type":"command","command":old_command}}).to_string(),
    );
    install(
        &temp.home(),
        &temp.data(),
        &new,
        RecorderParts {
            statusline: true,
            hooks: false,
        },
    )
    .expect("install");
    let state: Value =
        serde_json::from_slice(&fs::read(temp.data().join("recorder/state.json")).expect("state"))
            .expect("json");
    assert_eq!(state["original_statusline_present"], false);
    assert!(state["original_statusline"].is_null());
    uninstall(&temp.home(), &temp.data()).expect("uninstall");
    assert!(temp.read_settings().get("statusLine").is_none());
}

#[test]
fn executable_path_is_quoted_and_windows_slashes_are_normalized() {
    assert_eq!(
        normalize_executable_path("C:\\Program Files\\token-audit.exe"),
        "C:/Program Files/token-audit.exe"
    );
    let cmd = executable_command(
        Path::new("C:\\Program Files\\token-audit.exe"),
        "statusline",
    );
    #[cfg(windows)]
    assert_eq!(
        cmd.expect("command"),
        "\"C:/Program Files/token-audit.exe\" hook statusline"
    );
    #[cfg(unix)]
    assert!(cmd.is_err());
    let temp = TempDir::new();
    assert!(executable_command(&temp.exe(), "statusline")
        .expect("command")
        .starts_with('"'));
}

#[cfg(unix)]
#[test]
fn settings_permissions_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let temp = TempDir::new();
    temp.put_settings("{}");
    fs::set_permissions(temp.settings(), fs::Permissions::from_mode(0o600)).expect("mode");
    let plan = install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("install");
    let mode = fs::metadata(temp.settings())
        .expect("settings metadata")
        .permissions()
        .mode()
        & 0o777;
    let backup_mode = fs::metadata(plan.backup_path.expect("backup"))
        .expect("backup metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(backup_mode, 0o600);
}

#[test]
fn statusline_and_hooks_can_be_installed_independently() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"alpha":1}"#);
    install(
        &temp.home(),
        &temp.data(),
        &temp.exe(),
        RecorderParts {
            statusline: false,
            hooks: true,
        },
    )
    .expect("hooks install");
    assert!(temp.read_settings().get("statusLine").is_none());
    assert!(status(&temp.home(), &temp.data()).hooks_installed);
    install(
        &temp.home(),
        &temp.data(),
        &temp.exe(),
        RecorderParts {
            statusline: true,
            hooks: false,
        },
    )
    .expect("statusline install");
    let state = status(&temp.home(), &temp.data());
    assert!(state.hooks_installed && state.statusline_installed);
    uninstall(&temp.home(), &temp.data()).expect("uninstall");
    assert_eq!(temp.read_settings().to_string(), r#"{"alpha":1}"#);
}

#[test]
fn missing_state_uses_recorder_command_fallback() {
    let temp = TempDir::new();
    temp.put_settings("{}");
    install(&temp.home(), &temp.data(), &temp.exe(), both()).expect("install");
    fs::remove_file(temp.data().join("recorder/state.json")).expect("remove state");
    let plan = plan_uninstall(&temp.home(), &temp.data()).expect("fallback plan");
    assert_eq!(plan.changes.len(), 5);
    uninstall(&temp.home(), &temp.data()).expect("fallback uninstall");
    assert_eq!(temp.read_settings().to_string(), "{}");
}

#[test]
fn default_data_dir_keeps_simple_hook_command() {
    let temp = TempDir::new();
    #[cfg(target_os = "macos")]
    let data = temp.home().join("Library/Application Support/token-audit");
    #[cfg(windows)]
    let data = temp.home().join("AppData/Roaming/token-audit");
    #[cfg(not(any(target_os = "macos", windows)))]
    let data = temp.home().join(".local/share/token-audit");
    let plan = plan_install(&temp.home(), &data, &temp.exe(), both()).expect("plan");
    let status = plan
        .changes
        .iter()
        .find(|change| change.key_path == "statusLine")
        .expect("statusline");
    assert_eq!(
        status.after_command.as_deref(),
        Some(
            executable_command(&temp.exe(), "statusline")
                .expect("command")
                .as_str()
        )
    );
}

#[cfg(unix)]
#[test]
fn symlinked_settings_are_not_replaced() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new();
    fs::create_dir_all(temp.home().join(".claude")).expect("settings dir");
    let target = temp.0.join("source.json");
    fs::write(&target, "{}").expect("source");
    symlink(&target, temp.settings()).expect("link");
    assert!(install(&temp.home(), &temp.data(), &temp.exe(), both()).is_err());
    assert!(fs::symlink_metadata(temp.settings())
        .expect("link metadata")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(target).expect("source"), "{}");
}

#[test]
fn same_quota_in_new_session_is_recorded() {
    let temp = TempDir::new();
    let first = br#"{"session_id":"one","rate_limits":{"five_hour":{"used_percentage":28,"resets_at":1792795300}}}"#;
    let second = br#"{"session_id":"two","rate_limits":{"five_hour":{"used_percentage":28,"resets_at":1792795300}}}"#;
    record_statusline(&temp.data(), first);
    record_statusline(&temp.data(), second);
    assert_eq!(read_statusline_records(&temp.data()).0.len(), 2);
}
