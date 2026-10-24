use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

const EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "Stop", "SubagentStop"];
const INPUT_LIMIT: usize = 1024 * 1024;

#[derive(Debug)]
pub enum RecorderError {
    Io(io::Error),
    InvalidSettings,
    SymlinkSettings,
    InvalidState,
    InvalidExecutable,
}

impl std::fmt::Display for RecorderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "文件操作失败：{error}"),
            Self::InvalidSettings => write!(
                f,
                "Claude Code 配置不是有效的 JSON 对象或相关字段格式不正确"
            ),
            Self::SymlinkSettings => write!(f, "Claude Code 配置文件是符号链接，记录器不会改写"),
            Self::InvalidState => write!(f, "记录器状态文件格式不正确"),
            Self::InvalidExecutable => write!(f, "可执行文件路径必须是绝对路径"),
        }
    }
}
impl std::error::Error for RecorderError {}
impl From<io::Error> for RecorderError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecorderParts {
    pub statusline: bool,
    pub hooks: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedChange {
    pub file: PathBuf,
    pub key_path: String,
    pub action: &'static str,
    pub before_command: Option<String>,
    pub after_command: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecorderPlan {
    pub changes: Vec<PlannedChange>,
    pub backup_path: Option<PathBuf>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct InstallState {
    statusline_command: Option<String>,
    original_statusline: Option<Value>,
    original_statusline_present: bool,
    hooks_command: Option<String>,
    original_hooks_empty: bool,
    original_empty_hook_events: Vec<String>,
    settings_originally_existed: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RecorderStatus {
    pub statusline_installed: bool,
    pub statusline_modified: bool,
    pub hooks_installed: bool,
    pub hooks_modified: bool,
    pub statusline_records: usize,
    pub hook_events: usize,
    pub last_statusline_record: Option<DateTime<Utc>>,
    pub last_hook_event: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    pub used_percentage: Option<f64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptCache {
    pub warm: Option<bool>,
    pub ttl: Option<String>,
    pub expires_at: Option<i64>,
    pub hit_ratio: Option<f64>,
    pub misses: Option<u64>,
    pub last_miss_cause: Option<String>,
    pub recache_tokens_if_cold: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StatuslineRecord {
    pub recorded_at: DateTime<Utc>,
    pub session_id: Option<String>,
    pub version: Option<String>,
    pub model: Option<String>,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    pub prompt_cache: Option<PromptCache>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HookEvent {
    pub recorded_at: DateTime<Utc>,
    pub tool: String,
    pub event: String,
    pub session_id: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct ReadStats {
    pub total_lines: u64,
    pub malformed_lines: u64,
    pub unreadable_files: u64,
}

fn recorder_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("recorder")
}
fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude/settings.json")
}
fn state_path(data_dir: &Path) -> PathBuf {
    recorder_dir(data_dir).join("state.json")
}

pub fn normalize_executable_path(path: &str) -> String {
    path.replace('\\', "/")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}
pub fn executable_command(exe: &Path, suffix: &str) -> Result<String, RecorderError> {
    if !exe.is_absolute() {
        return Err(RecorderError::InvalidExecutable);
    }
    let path = normalize_executable_path(&exe.to_string_lossy());
    Ok(format!("\"{path}\" hook {suffix}"))
}

fn read_settings(home: &Path) -> Result<(Value, bool), RecorderError> {
    let path = settings_path(home);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(RecorderError::SymlinkSettings)
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Value::Object(Map::new()), false))
        }
        Err(error) => return Err(error.into()),
    }
    match fs::read(path) {
        Ok(bytes) => {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| RecorderError::InvalidSettings)?;
            if !value.is_object() {
                return Err(RecorderError::InvalidSettings);
            }
            Ok((value, true))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok((Value::Object(Map::new()), false))
        }
        Err(error) => Err(error.into()),
    }
}
fn read_state(data_dir: &Path) -> Result<InstallState, RecorderError> {
    match fs::read(state_path(data_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| RecorderError::InvalidState),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(InstallState::default()),
        Err(error) => Err(error.into()),
    }
}
fn command(value: &Value) -> Option<&str> {
    value.get("command")?.as_str()
}
fn installed_command(
    home: &Path,
    data_dir: &Path,
    exe: &Path,
    suffix: &str,
) -> Result<String, RecorderError> {
    let mut command = executable_command(exe, suffix)?;
    #[cfg(target_os = "macos")]
    let default_data_dir = home.join("Library/Application Support/token-audit");
    #[cfg(windows)]
    let default_data_dir = home.join("AppData/Roaming/token-audit");
    #[cfg(not(any(target_os = "macos", windows)))]
    let default_data_dir = home.join(".local/share/token-audit");
    if data_dir != default_data_dir {
        command.push_str(" --data-dir ");
        command.push('"');
        command.push_str(&normalize_executable_path(&data_dir.to_string_lossy()));
        command.push('"');
    }
    Ok(command)
}
fn fallback_ours(value: &str, suffix: &str) -> bool {
    let value = if let Some((base, arg)) = value.rsplit_once(" --data-dir ") {
        if arg.starts_with('"') && arg.ends_with('"') {
            base
        } else {
            return false;
        }
    } else {
        value
    };
    let Some(prefix) = value.strip_suffix(suffix) else {
        return false;
    };
    let Some(prefix) = prefix.strip_suffix(" hook ") else {
        return false;
    };
    let exe = if let Some(quoted) = prefix
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        quoted
    } else if !prefix.chars().any(char::is_whitespace) {
        prefix
    } else {
        return false;
    };
    let exe = exe.replace('\\', "/");
    matches!(
        exe.rsplit('/').next(),
        Some("token-audit" | "token-audit.exe")
    )
}
fn ours(value: &str, exact: Option<&str>, suffix: &str) -> bool {
    exact.is_some_and(|exact| value == exact) || fallback_ours(value, suffix)
}
fn hook_entry_ours(value: &Value, exact: Option<&str>) -> bool {
    value.get("matcher").and_then(Value::as_str) == Some("")
        && value
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.len() == 1
                    && hooks[0].get("type").and_then(Value::as_str) == Some("command")
                    && command(&hooks[0]).is_some_and(|cmd| ours(cmd, exact, "event --tool claude"))
            })
}
fn change(
    plan: &mut RecorderPlan,
    file: &Path,
    key: String,
    action: &'static str,
    before: Option<String>,
    after: Option<String>,
) {
    plan.changes.push(PlannedChange {
        file: file.to_path_buf(),
        key_path: key,
        action,
        before_command: before,
        after_command: after,
    });
}
fn new_plan() -> RecorderPlan {
    RecorderPlan {
        changes: Vec::new(),
        backup_path: None,
        notes: Vec::new(),
    }
}
fn backup_name(data_dir: &Path) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.9fZ");
    recorder_dir(data_dir)
        .join("backups")
        .join(format!("settings-{stamp}.json"))
}

struct Prepared {
    plan: RecorderPlan,
    settings: Value,
    state: InstallState,
    old_state: InstallState,
    existed: bool,
    remove_settings: bool,
}
fn prepare_install(
    home: &Path,
    data_dir: &Path,
    exe: &Path,
    parts: RecorderParts,
) -> Result<Prepared, RecorderError> {
    let (mut settings, existed) = read_settings(home)?;
    let mut state = read_state(data_dir)?;
    let old_state = state.clone();
    let file = settings_path(home);
    let mut plan = new_plan();
    let root = settings
        .as_object_mut()
        .ok_or(RecorderError::InvalidSettings)?;
    if parts.statusline {
        let desired = installed_command(home, data_dir, exe, "statusline")?;
        let current = root.get("statusLine").cloned();
        let current_cmd = current.as_ref().and_then(command).map(str::to_owned);
        let current_is_ours = current_cmd
            .as_deref()
            .is_some_and(|cmd| ours(cmd, state.statusline_command.as_deref(), "statusline"));
        if state
            .original_statusline
            .as_ref()
            .and_then(command)
            .is_some_and(|cmd| fallback_ours(cmd, "statusline"))
        {
            state.original_statusline = None;
            state.original_statusline_present = false;
        }
        if current_cmd.as_deref() != Some(desired.as_str()) {
            if let Some(Value::Object(ref old)) = current {
                let mut replacement = old.clone();
                replacement.insert("type".to_owned(), Value::String("command".to_owned()));
                replacement.insert("command".to_owned(), Value::String(desired.clone()));
                root.insert("statusLine".to_owned(), Value::Object(replacement));
            } else {
                root.insert(
                    "statusLine".to_owned(),
                    serde_json::json!({"type":"command","command":desired}),
                );
            }
            if !current_is_ours {
                state.original_statusline_present = current.is_some();
                state.original_statusline = current;
            }
            state.statusline_command = Some(desired.clone());
            change(
                &mut plan,
                &file,
                "statusLine".to_owned(),
                if current_is_ours {
                    "更新"
                } else if state.original_statusline_present {
                    "替换"
                } else {
                    "新增"
                },
                current_cmd,
                Some(desired),
            );
        } else if current_is_ours {
            state.statusline_command = Some(desired);
        }
    }
    if parts.hooks {
        let desired = installed_command(home, data_dir, exe, "event --tool claude")?;
        if !root.contains_key("hooks") {
            root.insert("hooks".to_owned(), Value::Object(Map::new()));
            state.original_hooks_empty = false;
        } else if root
            .get("hooks")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
            && state.hooks_command.is_none()
        {
            state.original_hooks_empty = true;
        }
        let hooks = root
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .ok_or(RecorderError::InvalidSettings)?;
        for event in EVENTS {
            let root_had_hook_event = hooks.contains_key(event);
            if !root_had_hook_event {
                hooks.insert(event.to_owned(), Value::Array(Vec::new()));
            }
            let entries = hooks
                .get_mut(event)
                .and_then(Value::as_array_mut)
                .ok_or(RecorderError::InvalidSettings)?;
            if entries.is_empty()
                && state.hooks_command.is_none()
                && !state
                    .original_empty_hook_events
                    .iter()
                    .any(|name| name == event)
                && root_had_hook_event
            {
                state.original_empty_hook_events.push(event.to_owned());
            }
            let mut found = false;
            for (index, entry) in entries.iter_mut().enumerate() {
                if !hook_entry_ours(entry, state.hooks_command.as_deref()) {
                    continue;
                }
                found = true;
                let Some(old_command) = entry
                    .get_mut("hooks")
                    .and_then(Value::as_array_mut)
                    .and_then(|list| list.first_mut())
                    .and_then(Value::as_object_mut)
                    .and_then(|hook| hook.get_mut("command"))
                else {
                    continue;
                };
                if old_command.as_str() != Some(desired.as_str()) {
                    let before = old_command.as_str().map(str::to_owned);
                    *old_command = Value::String(desired.clone());
                    change(
                        &mut plan,
                        &file,
                        format!("hooks.{event}[{index}]"),
                        "更新",
                        before,
                        Some(desired.clone()),
                    );
                }
            }
            if !found {
                let index = entries.len();
                entries.push(serde_json::json!({"matcher":"","hooks":[{"type":"command","command":desired}]}));
                change(
                    &mut plan,
                    &file,
                    format!("hooks.{event}[{index}]"),
                    "新增",
                    None,
                    Some(desired.clone()),
                );
            }
        }
        state.hooks_command = Some(desired);
    }
    if !plan.changes.is_empty() {
        plan.backup_path = Some(backup_name(data_dir));
        if state.settings_originally_existed.is_none() {
            state.settings_originally_existed = Some(existed);
        }
    }
    Ok(Prepared {
        plan,
        settings,
        state,
        old_state,
        existed,
        remove_settings: false,
    })
}

fn prepare_uninstall(
    home: &Path,
    data_dir: &Path,
    parts: RecorderParts,
) -> Result<Prepared, RecorderError> {
    let (mut settings, existed) = read_settings(home)?;
    let mut state = read_state(data_dir)?;
    let old_state = state.clone();
    let file = settings_path(home);
    let mut plan = new_plan();
    let root = settings
        .as_object_mut()
        .ok_or(RecorderError::InvalidSettings)?;
    if parts.statusline {
        if let Some(current) = root.get("statusLine").cloned() {
            if command(&current)
                .is_some_and(|cmd| ours(cmd, state.statusline_command.as_deref(), "statusline"))
            {
                let before = command(&current).map(str::to_owned);
                if state.original_statusline_present && state.original_statusline.is_none() {
                    plan.notes
                        .push("原 statusLine 备份缺失，保留当前配置".to_owned());
                } else if state.original_statusline_present
                    && !state
                        .original_statusline
                        .as_ref()
                        .and_then(command)
                        .is_some_and(|cmd| fallback_ours(cmd, "statusline"))
                {
                    if let Some(original) = state.original_statusline.take() {
                        let after = command(&original).map(str::to_owned);
                        root.insert("statusLine".to_owned(), original);
                        change(
                            &mut plan,
                            &file,
                            "statusLine".to_owned(),
                            "恢复",
                            before,
                            after,
                        );
                    }
                } else {
                    root.shift_remove("statusLine");
                    change(
                        &mut plan,
                        &file,
                        "statusLine".to_owned(),
                        "删除",
                        before,
                        None,
                    );
                }
            } else if state.statusline_command.is_some() {
                plan.notes
                    .push("statusLine 已被用户修改，保留当前配置".to_owned());
            }
        }
        state.statusline_command = None;
        state.original_statusline = None;
        state.original_statusline_present = false;
    }
    if parts.hooks {
        if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
            for event in EVENTS {
                if let Some(entries) = hooks.get_mut(event).and_then(Value::as_array_mut) {
                    for index in (0..entries.len()).rev() {
                        if hook_entry_ours(&entries[index], state.hooks_command.as_deref()) {
                            let before = entries[index]
                                .get("hooks")
                                .and_then(Value::as_array)
                                .and_then(|list| list.first())
                                .and_then(command)
                                .map(str::to_owned);
                            entries.remove(index);
                            change(
                                &mut plan,
                                &file,
                                format!("hooks.{event}[{index}]"),
                                "删除",
                                before,
                                None,
                            );
                        }
                    }
                    if entries.is_empty()
                        && !state
                            .original_empty_hook_events
                            .iter()
                            .any(|name| name == event)
                    {
                        hooks.shift_remove(event);
                    }
                }
            }
            if hooks.is_empty() && !state.original_hooks_empty {
                root.shift_remove("hooks");
            }
        }
        state.hooks_command = None;
        state.original_hooks_empty = false;
        state.original_empty_hook_events.clear();
    }
    let fully_removed = state.statusline_command.is_none() && state.hooks_command.is_none();
    let remove_settings = fully_removed
        && old_state.settings_originally_existed == Some(false)
        && settings.as_object().is_some_and(Map::is_empty);
    if fully_removed {
        state.settings_originally_existed = None;
    }
    if !plan.changes.is_empty() {
        plan.backup_path = Some(backup_name(data_dir));
    }
    Ok(Prepared {
        plan,
        settings,
        state,
        old_state,
        existed,
        remove_settings,
    })
}

pub fn plan_install(
    home: &Path,
    data_dir: &Path,
    exe: &Path,
    parts: RecorderParts,
) -> Result<RecorderPlan, RecorderError> {
    Ok(prepare_install(home, data_dir, exe, parts)?.plan)
}
pub fn plan_uninstall(
    home: &Path,
    data_dir: &Path,
    parts: RecorderParts,
) -> Result<RecorderPlan, RecorderError> {
    Ok(prepare_uninstall(home, data_dir, parts)?.plan)
}
fn pretty(value: &impl Serialize) -> Result<Vec<u8>, RecorderError> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|_| RecorderError::InvalidState)?;
    out.push(b'\n');
    Ok(out)
}
fn atomic_write(path: &Path, bytes: &[u8], original: Option<&Path>) -> Result<(), RecorderError> {
    let parent = path.parent().ok_or(RecorderError::InvalidSettings)?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".token-audit-{}-{}.tmp",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let result = (|| -> Result<(), RecorderError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Some(original) = original {
            if let Ok(metadata) = fs::metadata(original) {
                file.set_permissions(metadata.permissions())?;
            }
        }
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn backup_settings(home: &Path, backup: &Path, existed: bool) -> Result<(), RecorderError> {
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut destination = options.open(backup)?;
    if existed {
        let source = settings_path(home);
        let mut input = File::open(&source)?;
        let permissions = input.metadata()?.permissions();
        io::copy(&mut input, &mut destination)?;
        destination.sync_all()?;
        destination.set_permissions(permissions)?;
    }
    Ok(())
}
fn apply(home: &Path, data_dir: &Path, prepared: Prepared) -> Result<RecorderPlan, RecorderError> {
    if prepared.plan.changes.is_empty() {
        if prepared.state != prepared.old_state {
            let state_file = state_path(data_dir);
            atomic_write(&state_file, &pretty(&prepared.state)?, Some(&state_file))?;
        }
        return Ok(prepared.plan);
    }
    let backup = prepared
        .plan
        .backup_path
        .as_ref()
        .ok_or(RecorderError::InvalidState)?;
    backup_settings(home, backup, prepared.existed)?;
    let state_file = state_path(data_dir);
    atomic_write(&state_file, &pretty(&prepared.state)?, Some(&state_file))?;
    let settings_file = settings_path(home);
    let write_result = if prepared.remove_settings {
        fs::remove_file(&settings_file).map_err(RecorderError::from)
    } else {
        atomic_write(
            &settings_file,
            &pretty(&prepared.settings)?,
            Some(&settings_file),
        )
    };
    if let Err(error) = write_result {
        let _ = atomic_write(
            &state_file,
            &pretty(&prepared.old_state)?,
            Some(&state_file),
        );
        return Err(error);
    }
    Ok(prepared.plan)
}
pub fn install(
    home: &Path,
    data_dir: &Path,
    exe: &Path,
    parts: RecorderParts,
) -> Result<RecorderPlan, RecorderError> {
    apply(home, data_dir, prepare_install(home, data_dir, exe, parts)?)
}
pub fn uninstall(
    home: &Path,
    data_dir: &Path,
    parts: RecorderParts,
) -> Result<RecorderPlan, RecorderError> {
    apply(home, data_dir, prepare_uninstall(home, data_dir, parts)?)
}

fn append_record<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
    line.push(b'\n');
    if let Ok(mut existing) = File::open(path) {
        let len = existing.metadata()?.len();
        if len > 0 {
            existing.seek(SeekFrom::Start(len - 1))?;
            let mut last = [0];
            existing.read_exact(&mut last)?;
            if last[0] != b'\n' {
                line.insert(0, b'\n');
            }
        }
    }
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(&line)
}
fn last_complete_line(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(None);
    }
    let mut size = len.min(8192) as usize;
    loop {
        file.seek(SeekFrom::Start(len - size as u64))?;
        let mut tail = vec![0; size];
        file.read_exact(&mut tail)?;
        let end = if tail.last() == Some(&b'\n') {
            size - 1
        } else if let Some(index) = tail.iter().rposition(|byte| *byte == b'\n') {
            index
        } else if size < len as usize && size < INPUT_LIMIT {
            size = (size * 2).min(len as usize).min(INPUT_LIMIT);
            continue;
        } else {
            return Ok(None);
        };
        let start = tail[..end]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        if start == 0 && size < len as usize && size < INPUT_LIMIT {
            size = (size * 2).min(len as usize).min(INPUT_LIMIT);
            continue;
        }
        if start == end || (start == 0 && size < len as usize) {
            return Ok(None);
        }
        return Ok(Some(tail[start..end].to_vec()));
    }
}
fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}
fn quota_window(value: Option<&Value>) -> Option<QuotaWindow> {
    let value = value?.as_object()?;
    let used_percentage = value.get("used_percentage")?.as_f64()?;
    Some(QuotaWindow {
        used_percentage: Some(used_percentage),
        resets_at: value.get("resets_at").and_then(Value::as_i64),
    })
}
fn prompt_cache(value: Option<&Value>) -> Option<PromptCache> {
    let value = value?.as_object()?;
    Some(PromptCache {
        warm: value.get("warm").and_then(Value::as_bool),
        ttl: value.get("ttl").and_then(Value::as_str).map(str::to_owned),
        expires_at: value.get("expires_at").and_then(Value::as_i64),
        hit_ratio: value.get("hit_ratio").and_then(Value::as_f64),
        misses: value.get("misses").and_then(Value::as_u64),
        last_miss_cause: value
            .get("last_miss_cause")
            .and_then(Value::as_str)
            .map(str::to_owned),
        recache_tokens_if_cold: value.get("recache_tokens_if_cold").and_then(Value::as_u64),
    })
}
pub fn record_statusline(data_dir: &Path, input: &[u8]) {
    if input.len() > INPUT_LIMIT {
        return;
    }
    let Ok(raw) = serde_json::from_slice::<Value>(input) else {
        return;
    };
    let limits = raw.get("rate_limits");
    let five_hour = quota_window(limits.and_then(|value| value.get("five_hour")));
    let seven_day = quota_window(limits.and_then(|value| value.get("seven_day")));
    if five_hour.is_none() && seven_day.is_none() {
        return;
    }
    let path = recorder_dir(data_dir).join("statusline.jsonl");
    let record = StatuslineRecord {
        recorded_at: Utc::now(),
        session_id: string_field(&raw, "session_id"),
        version: string_field(&raw, "version"),
        model: raw.get("model").and_then(|value| string_field(value, "id")),
        five_hour,
        seven_day,
        prompt_cache: prompt_cache(raw.get("prompt_cache")),
    };
    if let Ok(Some(line)) = last_complete_line(&path) {
        if let Ok(last) = serde_json::from_slice::<StatuslineRecord>(&line) {
            if last.session_id == record.session_id
                && last.five_hour == record.five_hour
                && last.seven_day == record.seven_day
            {
                return;
            }
        }
    }
    let _ = append_record(&path, &record);
}
pub fn record_hook_event(data_dir: &Path, input: &[u8]) {
    if input.len() > INPUT_LIMIT {
        return;
    }
    let Ok(raw) = serde_json::from_slice::<Value>(input) else {
        return;
    };
    let Some(event) =
        string_field(&raw, "hook_event_name").filter(|event| EVENTS.contains(&event.as_str()))
    else {
        return;
    };
    let record = HookEvent {
        recorded_at: Utc::now(),
        tool: "claude".to_owned(),
        event,
        session_id: string_field(&raw, "session_id"),
    };
    let _ = append_record(&recorder_dir(data_dir).join("hooks.jsonl"), &record);
}
fn read_records<T: DeserializeOwned>(path: &Path) -> (Vec<T>, ReadStats) {
    let mut stats = ReadStats::default();
    let mut records = Vec::new();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (records, stats),
        Err(_) => {
            stats.unreadable_files = 1;
            return (records, stats);
        }
    };
    for line in BufReader::new(file).split(b'\n') {
        stats.total_lines += 1;
        match line {
            Ok(bytes) if bytes.is_empty() => {
                stats.total_lines -= 1;
            }
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(record) => records.push(record),
                Err(_) => stats.malformed_lines += 1,
            },
            Err(_) => {
                stats.malformed_lines += 1;
                break;
            }
        }
    }
    (records, stats)
}
pub fn read_statusline_records(data_dir: &Path) -> (Vec<StatuslineRecord>, ReadStats) {
    let (mut records, stats): (Vec<StatuslineRecord>, ReadStats) =
        read_records(&recorder_dir(data_dir).join("statusline.jsonl"));
    records.sort_by_key(|record| record.recorded_at);
    (records, stats)
}
pub fn read_hook_events(data_dir: &Path) -> (Vec<HookEvent>, ReadStats) {
    let (mut records, stats): (Vec<HookEvent>, ReadStats) =
        read_records(&recorder_dir(data_dir).join("hooks.jsonl"));
    records.sort_by_key(|record| record.recorded_at);
    (records, stats)
}
pub fn original_statusline_command(data_dir: &Path) -> Option<String> {
    read_state(data_dir)
        .ok()?
        .original_statusline
        .as_ref()
        .and_then(command)
        .filter(|cmd| !fallback_ours(cmd, "statusline"))
        .map(str::to_owned)
}
pub fn status(home: &Path, data_dir: &Path) -> RecorderStatus {
    let mut result = RecorderStatus::default();
    let state = read_state(data_dir).unwrap_or_default();
    result.statusline_modified = state.statusline_command.is_some();
    result.hooks_modified = state.hooks_command.is_some();
    if let Ok((settings, _)) = read_settings(home) {
        let current = settings.get("statusLine").and_then(command);
        result.statusline_installed =
            current.is_some_and(|cmd| ours(cmd, state.statusline_command.as_deref(), "statusline"));
        result.statusline_modified =
            state.statusline_command.is_some() && !result.statusline_installed;
        if let Some(hooks) = settings.get("hooks").and_then(Value::as_object) {
            let count = EVENTS
                .iter()
                .filter(|event| {
                    hooks
                        .get(**event)
                        .and_then(Value::as_array)
                        .is_some_and(|entries| {
                            entries
                                .iter()
                                .any(|entry| hook_entry_ours(entry, state.hooks_command.as_deref()))
                        })
                })
                .count();
            result.hooks_installed = count == EVENTS.len();
            result.hooks_modified = state.hooks_command.is_some() && count != EVENTS.len();
        } else {
            result.hooks_modified = state.hooks_command.is_some();
        }
    }
    let (statuslines, _) = read_statusline_records(data_dir);
    let (events, _) = read_hook_events(data_dir);
    result.statusline_records = statuslines.len();
    result.hook_events = events.len();
    result.last_statusline_record = statuslines.last().map(|r| r.recorded_at);
    result.last_hook_event = events.last().map(|r| r.recorded_at);
    result
}
pub fn clear_records(data_dir: &Path) -> Result<(), RecorderError> {
    for name in ["statusline.jsonl", "hooks.jsonl"] {
        match fs::remove_file(recorder_dir(data_dir).join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
