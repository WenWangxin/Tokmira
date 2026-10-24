use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Output;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{Duration, Instant};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "token-audit-cli-recorder-test-{}-{}",
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
    #[cfg(unix)]
    fn put_settings(&self, text: &str) {
        fs::create_dir_all(self.home().join(".claude")).expect("settings dir");
        fs::write(self.settings(), text).expect("settings");
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temp dir");
    }
}
fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_token-audit")
}
#[cfg(unix)]
fn install(temp: &TempDir) {
    let output = Command::new(binary())
        .args(["recorder", "install", "--statusline", "--home"])
        .arg(temp.home())
        .arg("--data-dir")
        .arg(temp.data())
        .arg("--exe")
        .arg(binary())
        .output()
        .expect("install command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(unix)]
fn hook(temp: &TempDir, data: &[u8]) -> Output {
    let mut child = Command::new(binary())
        .args(["hook", "statusline", "--data-dir"])
        .arg(temp.data())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("hook");
    let mut stdin = child.stdin.take().expect("stdin");
    let input = data.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().expect("hook output");
    writer.join().expect("writer thread").expect("stdin write");
    output
}

#[cfg(unix)]
#[test]
fn original_statusline_receives_exact_stdin_and_stdout_is_unchanged() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"cat","padding":0}}"#);
    install(&temp);
    let data = include_bytes!("../../core/tests/fixtures/recorder/statusline.json");
    let output = hook(&temp, data);
    assert!(output.status.success());
    assert_eq!(output.stdout, data);
    assert!(output.stderr.is_empty());
    let records =
        fs::read_to_string(temp.data().join("recorder/statusline.jsonl")).expect("records");
    assert!(records.contains("five_hour"));
    assert!(!records.contains("PRIVATE_"));
    let installed: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.settings()).expect("settings")).expect("json");
    let installed_command = installed["statusLine"]["command"]
        .as_str()
        .expect("command");
    assert!(installed_command.contains("--data-dir"));
    let mut launched = Command::new("/bin/sh")
        .arg("-c")
        .arg(installed_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("launch installed command");
    let mut stdin = launched.stdin.take().expect("stdin");
    let input = data.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let launched = launched.wait_with_output().expect("output");
    writer.join().expect("writer thread").expect("write");
    assert_eq!(launched.stdout, data);
    let oversized = vec![b'x'; 1024 * 1024 + 10];
    let oversized_output = hook(&temp, &oversized);
    assert_eq!(oversized_output.stdout, oversized);
    assert_eq!(
        fs::read_to_string(temp.data().join("recorder/statusline.jsonl"))
            .expect("records")
            .lines()
            .count(),
        1
    );
    fs::create_dir(temp.data().join("recorder/hooks.jsonl")).expect("unrelated failure path");
    fs::remove_file(temp.data().join("recorder/statusline.jsonl")).expect("remove records");
    fs::create_dir(temp.data().join("recorder/statusline.jsonl")).expect("unwritable record path");
    let failed_recording = hook(&temp, data);
    assert!(failed_recording.status.success());
    assert_eq!(failed_recording.stdout, data);
    assert!(failed_recording.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn original_statusline_timeout_is_bounded() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"sleep 10 & wait"}}"#);
    install(&temp);
    let started = Instant::now();
    let output = hook(&temp, b"{}");
    assert!(!output.status.success());
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[test]
fn event_hook_is_silent_and_filters_prompt() {
    let temp = TempDir::new();
    let mut child = Command::new(binary())
        .args(["hook", "event", "--tool", "claude", "--data-dir"])
        .arg(temp.data())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("event hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(include_bytes!(
            "../../core/tests/fixtures/recorder/event.json"
        ))
        .expect("write");
    let output = child.wait_with_output().expect("output");
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let records = fs::read_to_string(temp.data().join("recorder/hooks.jsonl")).expect("records");
    assert!(records.contains("UserPromptSubmit"));
    assert!(!records.contains("PRIVATE_") && !records.contains("prompt"));
}

#[test]
fn dry_run_does_not_write_settings_or_backups() {
    let temp = TempDir::new();
    let output = Command::new(binary())
        .args(["recorder", "install", "--dry-run", "--home"])
        .arg(temp.home())
        .arg("--data-dir")
        .arg(temp.data())
        .arg("--exe")
        .arg(Path::new(binary()))
        .output()
        .expect("dry run");
    assert!(output.status.success());
    assert!(!temp.settings().exists());
    assert!(!temp.data().exists());
}

#[cfg(unix)]
#[test]
fn original_statusline_exit_code_is_preserved() {
    let temp = TempDir::new();
    temp.put_settings(r#"{"statusLine":{"type":"command","command":"exit 7"}}"#);
    install(&temp);
    let output = hook(&temp, b"{}");
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn own_original_statusline_is_not_launched() {
    let temp = TempDir::new();
    install(&temp);
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.settings()).expect("settings")).expect("json");
    let own_command = settings["statusLine"]["command"].clone();
    let state_path = temp.data().join("recorder/state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("state")).expect("json");
    state["original_statusline_present"] = serde_json::Value::Bool(true);
    state["original_statusline"] = serde_json::json!({"type":"command","command":own_command});
    fs::write(&state_path, serde_json::to_vec(&state).expect("serialize")).expect("state");
    let started = Instant::now();
    let output = hook(
        &temp,
        include_bytes!("../../core/tests/fixtures/recorder/statusline.json"),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn chained_command_cannot_recurse_through_hook() {
    let temp = TempDir::new();
    let script = temp.0.join("original.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '{{}}' | \"{}\" hook statusline --data-dir \"{}\"\nprintf done",
            binary(),
            temp.data().display()
        ),
    )
    .expect("script");
    temp.put_settings(&serde_json::json!({"statusLine":{"type":"command","command":format!("/bin/sh {}", script.display())}}).to_string());
    install(&temp);
    let started = Instant::now();
    let output = hook(
        &temp,
        include_bytes!("../../core/tests/fixtures/recorder/statusline.json"),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"done");
    assert_eq!(
        fs::read_to_string(temp.data().join("recorder/statusline.jsonl"))
            .expect("records")
            .lines()
            .count(),
        1
    );
}

#[test]
fn records_display_local_record_and_reset_times() {
    let temp = TempDir::new();
    let mut child = Command::new(binary())
        .args(["hook", "statusline", "--data-dir"])
        .arg(temp.data())
        .stdin(Stdio::piped())
        .spawn()
        .expect("hook");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(include_bytes!(
            "../../core/tests/fixtures/recorder/statusline.json"
        ))
        .expect("write");
    assert!(child.wait().expect("wait").success());
    let output = Command::new(binary())
        .args(["recorder", "records", "--home"])
        .arg(temp.home())
        .arg("--data-dir")
        .arg(temp.data())
        .output()
        .expect("records");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("utf8");
    let reset = chrono::DateTime::from_timestamp(1792795300, 0)
        .expect("timestamp")
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert!(text.contains(&reset));
    assert!(!text.contains("1792795300"));
    let first = text.lines().next().expect("first line");
    assert!(first.chars().take(16).enumerate().all(|(i, c)| match i {
        4 | 7 => c == '-',
        10 => c == ' ',
        13 => c == ':',
        _ => c.is_ascii_digit(),
    }));
}

#[cfg(unix)]
#[test]
fn uninstall_flags_remove_only_the_selected_part() {
    let temp = TempDir::new();
    fs::create_dir_all(temp.home().join(".claude")).expect("settings dir");
    fs::write(
        temp.settings(),
        r#"{"statusLine":{"type":"command","command":"printf old"}}"#,
    )
    .expect("settings");
    let install = Command::new(binary())
        .args(["recorder", "install", "--statusline", "--hooks", "--home"])
        .arg(temp.home())
        .arg("--data-dir")
        .arg(temp.data())
        .arg("--exe")
        .arg(binary())
        .output()
        .expect("install");
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let uninstall = Command::new(binary())
        .args(["recorder", "uninstall", "--hooks", "--home"])
        .arg(temp.home())
        .arg("--data-dir")
        .arg(temp.data())
        .output()
        .expect("uninstall");
    assert!(
        uninstall.status.success(),
        "{}",
        String::from_utf8_lossy(&uninstall.stderr)
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.settings()).expect("settings")).expect("json");
    assert!(settings["statusLine"]["command"]
        .as_str()
        .expect("command")
        .contains("statusline"));
    assert!(settings.get("hooks").is_none());
}
