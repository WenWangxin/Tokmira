//! "Open session" and "fill command": brings a coding session back to the front and, where
//! the terminal allows it, types a slash command without pressing Enter. The user always
//! sends it. When the exact tab cannot be told apart, nothing is typed and the command is
//! copied instead.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use token_audit_core::advice::AdviceAlert;
use token_audit_core::model::{Source, Tool};
use token_audit_core::recorder;
use token_audit_core::session_open::{
    can_fill, claude_desktop_link, claude_desktop_session, codex_desktop_link, is_cli_process,
    is_fill_command, pick_process, resume_command, Candidate, OpenOutcome, Pick, TerminalEnv,
    TerminalKind,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub tool: Tool,
    pub source: Source,
    pub session_id: String,
    pub project: Option<String>,
}

/// What a notification button carries back: the session and the command it may type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPayload {
    pub target: Target,
    pub command: Option<String>,
}

impl ActionPayload {
    pub fn for_alert(alert: &AdviceAlert) -> Self {
        Self {
            target: Target {
                tool: alert.session.tool,
                source: alert.session.source,
                session_id: alert.session.session_id.clone(),
                project: alert.session.project.clone(),
            },
            command: alert.command.clone(),
        }
    }
}

/// A CLI process found running the session.
struct Live {
    pid: Pid,
    env: TerminalEnv,
}

pub fn open(
    app: &AppHandle,
    data_dir: &Path,
    target: &Target,
    command: Option<&str>,
) -> OpenOutcome {
    let command = command.filter(|command| is_fill_command(command));
    let copy = |command: Option<&str>| {
        if let Some(command) = command {
            let _ = app.clipboard().write_text(command);
        }
    };
    let opened_with = |shown: bool| match (shown, command) {
        (true, Some(_)) => OpenOutcome::FocusedCopied,
        (true, None) => OpenOutcome::Focused,
        (false, Some(_)) => OpenOutcome::AppCopied,
        (false, None) => OpenOutcome::AppOpened,
    };

    match target.source {
        Source::ClaudeDesktop => {
            copy(command);
            let link = claude_desktop_sessions_dir()
                .and_then(|dir| claude_desktop_session(&dir, &target.session_id))
                .and_then(|id| claude_desktop_link(&id));
            return match link {
                Some(link) if open_url(&link) => opened_with(true),
                _ if activate_app(CLAUDE_DESKTOP) => opened_with(false),
                _ => OpenOutcome::NotFound,
            };
        }
        Source::CodexDesktop => {
            copy(command);
            return match codex_desktop_link(&target.session_id) {
                Some(link) if open_url(&link) => opened_with(true),
                _ if activate_app(CODEX_DESKTOP) => opened_with(false),
                _ => OpenOutcome::NotFound,
            };
        }
        Source::ClaudeCli | Source::CodexCli | Source::Unknown => {}
    }

    let Some(project) = target.project.as_deref().map(PathBuf::from) else {
        copy(command);
        return OpenOutcome::NotFound;
    };
    let recorded = (target.tool == Tool::Claude)
        .then(|| recorder::recorded_terminal(data_dir, &target.session_id))
        .flatten();
    let (live, others) = find_live(target, &project, recorded.as_ref());
    if let Some(live) = live {
        let filled =
            command.is_some_and(|command| can_fill(&live.env) && fill(&live, &project, command));
        if filled {
            return OpenOutcome::Filled;
        }
        copy(command);
        return opened_with(focus(&live, &project));
    }
    copy(command);
    if let Some(env) = others {
        // Several sessions share the folder: bring their terminal forward, type nothing.
        return if activate_terminal(&env, &project) {
            opened_with(false)
        } else {
            OpenOutcome::NotFound
        };
    }
    let kind = recorded
        .as_ref()
        .map_or(TerminalKind::Unknown, TerminalEnv::kind);
    match resume_command(target.tool, &target.session_id) {
        Some(resume)
            if project.is_dir() && resume_in(kind, recorded.as_ref(), &project, &resume) =>
        {
            OpenOutcome::Resumed
        }
        _ => OpenOutcome::NotFound,
    }
}

/// The live process for the session, or, when several share the folder and none can be told
/// apart, the terminal of one of them.
fn find_live(
    target: &Target,
    project: &Path,
    recorded: Option<&TerminalEnv>,
) -> (Option<Live>, Option<TerminalEnv>) {
    let Ok(mut system) = System::new() else {
        return (None, None);
    };
    // First pass: program, arguments and folder only. Environments hold other programs' API
    // keys, so they are read below, and only for the few processes that match.
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    let mut pids = Vec::new();
    let mut mentions = Vec::new();
    for (pid, process) in system.processes() {
        let exe = process
            .exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| process.name().to_string_lossy().into_owned());
        let args: Vec<String> = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        if is_cli_process(target.tool, &exe, &args)
            && process.cwd().is_some_and(|cwd| same_dir(cwd, project))
        {
            pids.push(*pid);
            mentions.push(args.iter().any(|arg| arg == &target.session_id));
        }
    }
    if !pids.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            false,
            ProcessRefreshKind::nothing().with_environ(UpdateKind::Always),
        );
    }
    let mut candidates: Vec<Candidate> = pids
        .iter()
        .zip(mentions)
        .map(|(pid, mentions_session)| Candidate {
            mentions_session,
            // Only the allowlisted terminal variables are copied out.
            env: system
                .process(*pid)
                .map_or_else(TerminalEnv::default, |process| {
                    TerminalEnv::capture(|key| {
                        process.environ().iter().find_map(|entry| {
                            let entry = entry.to_string_lossy();
                            entry
                                .strip_prefix(key)
                                .and_then(|rest| rest.strip_prefix('='))
                                .map(str::to_owned)
                        })
                    })
                }),
        })
        .collect();
    match pick_process(&candidates, recorded) {
        Pick::Exact(index) | Pick::Only(index) => (
            Some(Live {
                pid: pids[index],
                env: candidates.swap_remove(index).env,
            }),
            None,
        ),
        Pick::Ambiguous => (None, candidates.into_iter().next().map(|item| item.env)),
        Pick::None => (None, None),
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical(a) == canonical(b)
}

#[cfg(target_os = "macos")]
const CLAUDE_DESKTOP: &str = "com.anthropic.claudefordesktop";
#[cfg(target_os = "macos")]
const CODEX_DESKTOP: &str = "com.openai.codex";
#[cfg(not(target_os = "macos"))]
const CLAUDE_DESKTOP: &str = "claude://";
#[cfg(not(target_os = "macos"))]
const CODEX_DESKTOP: &str = "codex://";

/// `~/Library/Application Support/Claude/claude-code-sessions` on macOS and, by the same
/// layout, `%APPDATA%\Claude\claude-code-sessions` on Windows.
fn claude_desktop_sessions_dir() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("Claude")
            .join("claude-code-sessions"),
    )
}

#[cfg_attr(windows, allow(dead_code))]
fn run(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `"…"` for AppleScript.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `'…'` for a POSIX shell.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Option<String> {
    run("osascript", &["-e", script])
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> bool {
    run("open", &[url]).is_some()
}

#[cfg(windows)]
fn open_url(url: &str) -> bool {
    // Explorer hands the link to its registered app; links are built from checked ids.
    Command::new("explorer.exe").arg(url).spawn().is_ok()
}

#[cfg(target_os = "macos")]
fn activate_app(bundle: &str) -> bool {
    run("open", &["-b", bundle]).is_some()
}

#[cfg(windows)]
fn activate_app(link: &str) -> bool {
    open_url(link)
}

// --- macOS -----------------------------------------------------------------

#[cfg(target_os = "macos")]
const GHOSTTY: &str = "com.mitchellh.ghostty";
#[cfg(target_os = "macos")]
const ITERM: &str = "com.googlecode.iterm2";
#[cfg(target_os = "macos")]
const APPLE_TERMINAL: &str = "com.apple.Terminal";
#[cfg(target_os = "macos")]
const WEZTERM: &str = "com.github.wez.wezterm";

#[cfg(target_os = "macos")]
fn bundle_for(env: &TerminalEnv) -> Option<String> {
    if let Some(bundle) = env.bundle_id() {
        return Some(bundle.to_owned());
    }
    Some(
        match env.kind() {
            TerminalKind::ITerm => ITERM,
            TerminalKind::Ghostty => GHOSTTY,
            TerminalKind::AppleTerminal => APPLE_TERMINAL,
            TerminalKind::WezTerm => WEZTERM,
            TerminalKind::Kitty => "net.kovidgoyal.kitty",
            TerminalKind::VsCode => "com.microsoft.VSCode",
            TerminalKind::Zed => "dev.zed.Zed",
            _ => return None,
        }
        .to_owned(),
    )
}

#[cfg(target_os = "macos")]
fn tmux(socket: &str, args: &[&str]) -> bool {
    let mut all = vec!["-S", socket];
    all.extend_from_slice(args);
    run("tmux", &all).is_some()
        || ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux"]
            .iter()
            .any(|path| Path::new(path).exists() && run(path, &all).is_some())
}

#[cfg(target_os = "macos")]
fn tool_path(name: &str, bundled: &str) -> PathBuf {
    if Path::new(bundled).exists() {
        PathBuf::from(bundled)
    } else {
        PathBuf::from(name)
    }
}

#[cfg(target_os = "macos")]
fn wezterm(args: &[&str]) -> Option<String> {
    run(
        tool_path(
            "wezterm",
            "/Applications/WezTerm.app/Contents/MacOS/wezterm",
        ),
        args,
    )
}

#[cfg(target_os = "macos")]
fn kitten(listen: &str, args: &[&str]) -> bool {
    let mut all = vec!["@", "--to", listen];
    all.extend_from_slice(args);
    run(
        tool_path("kitten", "/Applications/kitty.app/Contents/MacOS/kitten"),
        &all,
    )
    .is_some()
}

/// Types `command` into the session's tab and brings it forward. False when the terminal
/// refused or the tab could not be pinned down; nothing has been typed then.
#[cfg(target_os = "macos")]
fn fill(live: &Live, project: &Path, command: &str) -> bool {
    let env = &live.env;
    if let Some((socket, pane)) = env.tmux() {
        let typed = tmux(socket, &["select-window", "-t", pane])
            && tmux(socket, &["select-pane", "-t", pane])
            && tmux(socket, &["send-keys", "-t", pane, "-l", command]);
        if typed {
            if let Some(bundle) = bundle_for(env) {
                activate_app(&bundle);
            }
        }
        return typed;
    }
    match env.kind() {
        TerminalKind::ITerm => iterm(env, Some(command)),
        TerminalKind::Ghostty => ghostty(project, Some(command)),
        TerminalKind::WezTerm => {
            let Some(pane) = env.wezterm_pane().map(|pane| pane.to_string()) else {
                return false;
            };
            let typed = wezterm(&["cli", "activate-pane", "--pane-id", &pane]).is_some()
                && wezterm(&[
                    "cli",
                    "send-text",
                    "--pane-id",
                    &pane,
                    "--no-paste",
                    "--",
                    command,
                ])
                .is_some();
            if typed {
                activate_app(WEZTERM);
            }
            typed
        }
        TerminalKind::Kitty => {
            let (Some(window), Some(listen)) = (env.kitty_window(), env.get("KITTY_LISTEN_ON"))
            else {
                return false;
            };
            let id = format!("id:{window}");
            let typed = kitten(listen, &["focus-window", "--match", &id])
                && kitten(listen, &["send-text", "--match", &id, "--", command]);
            if typed {
                activate_app("net.kovidgoyal.kitty");
            }
            typed
        }
        _ => false,
    }
}

/// Brings the session's tab (or at least its app) forward. True when the exact tab is shown.
#[cfg(target_os = "macos")]
fn focus(live: &Live, project: &Path) -> bool {
    let env = &live.env;
    if let Some((socket, pane)) = env.tmux() {
        let shown = tmux(socket, &["select-window", "-t", pane])
            && tmux(socket, &["select-pane", "-t", pane]);
        if let Some(bundle) = bundle_for(env) {
            activate_app(&bundle);
        }
        return shown;
    }
    let shown = match env.kind() {
        TerminalKind::ITerm => iterm(env, None),
        TerminalKind::Ghostty => ghostty(project, None),
        TerminalKind::AppleTerminal => apple_terminal(live.pid),
        TerminalKind::WezTerm => env.wezterm_pane().is_some_and(|pane| {
            wezterm(&["cli", "activate-pane", "--pane-id", &pane.to_string()]).is_some()
        }),
        TerminalKind::Kitty => match (env.kitty_window(), env.get("KITTY_LISTEN_ON")) {
            (Some(window), Some(listen)) => kitten(
                listen,
                &["focus-window", "--match", &format!("id:{window}")],
            ),
            _ => false,
        },
        _ => false,
    };
    if !shown {
        activate_terminal(env, project);
    } else if matches!(env.kind(), TerminalKind::WezTerm | TerminalKind::Kitty) {
        if let Some(bundle) = bundle_for(env) {
            activate_app(&bundle);
        }
    }
    shown
}

/// The app only. Editors open the project's window, which holds its terminal panel.
#[cfg(target_os = "macos")]
fn activate_terminal(env: &TerminalEnv, project: &Path) -> bool {
    let Some(bundle) = bundle_for(env) else {
        return false;
    };
    let editor = matches!(
        env.kind(),
        TerminalKind::VsCode | TerminalKind::Cursor | TerminalKind::Windsurf | TerminalKind::Zed
    );
    if editor {
        if let Some(project) = project.to_str() {
            return run("open", &["-b", &bundle, project]).is_some();
        }
    }
    activate_app(&bundle)
}

#[cfg(target_os = "macos")]
fn iterm(env: &TerminalEnv, command: Option<&str>) -> bool {
    let Some(id) = env.iterm_session() else {
        return false;
    };
    let write = command.map_or(String::new(), |command| {
        format!(
            "tell s to write text {} newline no\n",
            applescript_string(command)
        )
    });
    let script = format!(
        r#"tell application id "{ITERM}"
repeat with w in windows
repeat with t in tabs of w
repeat with s in sessions of t
if unique id of s is {id} then
select w
tell t to select
tell s to select
{write}activate
return "ok"
end if
end repeat
end repeat
end repeat
end tell
return "missing""#,
        id = applescript_string(id),
    );
    osascript(&script).as_deref() == Some("ok")
}

/// Ghostty does not expose a tab id to the shell, so the tab is found by its folder; two
/// tabs in the same folder count as "not found".
#[cfg(target_os = "macos")]
fn ghostty(project: &Path, command: Option<&str>) -> bool {
    let Some(project) = project.to_str() else {
        return false;
    };
    let input = command.map_or(String::new(), |command| {
        format!("input text {} to t\n", applescript_string(command))
    });
    let script = format!(
        r#"tell application id "{GHOSTTY}"
set found to every terminal whose working directory is {dir}
if (count of found) is not 1 then return "ambiguous"
set t to item 1 of found
focus t
{input}activate
return "ok"
end tell"#,
        dir = applescript_string(project),
    );
    osascript(&script).as_deref() == Some("ok")
}

/// Terminal tabs are found by tty. Terminal has no way to type without running a command,
/// so the caller copies the command instead.
#[cfg(target_os = "macos")]
fn apple_terminal(pid: Pid) -> bool {
    let Some(tty) = run("ps", &["-o", "tty=", "-p", &pid.to_string()])
        .filter(|tty| tty.starts_with("ttys") && tty[4..].chars().all(|ch| ch.is_ascii_digit()))
    else {
        return false;
    };
    let script = format!(
        r#"tell application id "{APPLE_TERMINAL}"
repeat with w in windows
repeat with t in tabs of w
if tty of t is "/dev/{tty}" then
set selected of t to true
set index of w to 1
activate
return "ok"
end if
end repeat
end repeat
end tell
return "missing""#
    );
    osascript(&script).as_deref() == Some("ok")
}

/// Opens a new window in the project folder that runs `resume`, in the terminal the session
/// last used when Tokmira can start one there, otherwise in Terminal.
#[cfg(target_os = "macos")]
fn resume_in(kind: TerminalKind, env: Option<&TerminalEnv>, project: &Path, resume: &str) -> bool {
    let Some(dir) = project.to_str() else {
        return false;
    };
    let line = format!("cd {} && {resume}", shell_quote(dir));
    let started = match kind {
        TerminalKind::Ghostty => osascript(&format!(
            r#"tell application id "{GHOSTTY}"
new window with configuration {{initial working directory:{dir}, initial input:{input}}}
activate
end tell"#,
            dir = applescript_string(dir),
            input = applescript_string(&format!("{resume}\n")),
        ))
        .is_some(),
        TerminalKind::ITerm => osascript(&format!(
            r#"tell application id "{ITERM}"
set w to (create window with default profile)
tell current session of w to write text {line}
activate
end tell"#,
            line = applescript_string(&line),
        ))
        .is_some(),
        TerminalKind::WezTerm => wezterm(&["cli", "spawn", "--new-window", "--cwd", dir])
            .filter(|pane| pane.chars().all(|ch| ch.is_ascii_digit()) && !pane.is_empty())
            .is_some_and(|pane| {
                let typed = wezterm(&[
                    "cli",
                    "send-text",
                    "--pane-id",
                    &pane,
                    "--no-paste",
                    "--",
                    &format!("{resume}\n"),
                ])
                .is_some();
                activate_app(WEZTERM);
                typed
            }),
        TerminalKind::Kitty => {
            env.and_then(|env| env.get("KITTY_LISTEN_ON"))
                .is_some_and(|listen| {
                    let cwd = format!("--cwd={dir}");
                    kitten(listen, &["launch", "--type=os-window", &cwd])
                        && kitten(
                            listen,
                            &[
                                "send-text",
                                "--match",
                                "recent:0",
                                "--",
                                &format!("{resume}\n"),
                            ],
                        )
                        && activate_app("net.kovidgoyal.kitty")
                })
        }
        _ => false,
    };
    started
        || osascript(&format!(
            r#"tell application id "{APPLE_TERMINAL}"
do script {line}
activate
end tell"#,
            line = applescript_string(&line),
        ))
        .is_some()
}

// --- Windows ---------------------------------------------------------------

/// Types into the session's console input, which reaches the right tab even when Windows
/// Terminal shows another one.
#[cfg(windows)]
fn fill(live: &Live, _project: &Path, command: &str) -> bool {
    let typed = win::write_console_input(live.pid.as_u32(), command);
    if typed {
        focus(live, _project);
    }
    typed
}

#[cfg(windows)]
fn focus(live: &Live, _project: &Path) -> bool {
    let Ok(mut system) = System::new() else {
        return win::focus_console(live.pid.as_u32());
    };
    system.refresh_processes(ProcessesToUpdate::All, true);
    // The console window for classic consoles; the hosting Windows Terminal (or editor)
    // otherwise, found by walking up the process tree.
    let mut chain = Vec::new();
    let mut current = Some(live.pid);
    while let Some(pid) = current {
        chain.push(pid.as_u32());
        current = system.process(pid).and_then(sysinfo::Process::parent);
        if chain.len() > 16 {
            break;
        }
    }
    win::focus_console(live.pid.as_u32()) || chain.iter().any(|pid| win::focus_process_window(*pid))
}

#[cfg(windows)]
fn activate_terminal(_env: &TerminalEnv, _project: &Path) -> bool {
    false
}

/// Windows Terminal when installed (`wt -d <dir> cmd /k <resume>`), otherwise a console.
#[cfg(windows)]
fn resume_in(
    _kind: TerminalKind,
    _env: Option<&TerminalEnv>,
    project: &Path,
    resume: &str,
) -> bool {
    let args: Vec<&str> = resume.split(' ').collect();
    let mut wt = Command::new("wt.exe");
    wt.arg("-d").arg(project).arg("cmd").arg("/k").args(&args);
    if wt.spawn().is_ok() {
        return true;
    }
    Command::new("cmd")
        .arg("/c")
        .arg("start")
        .arg("")
        .arg("/D")
        .arg(project)
        .arg("cmd")
        .arg("/k")
        .args(&args)
        .spawn()
        .is_ok()
}

#[cfg(windows)]
mod win {
    use std::ptr;

    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HWND, INVALID_HANDLE_VALUE, LPARAM,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GetConsoleWindow, WriteConsoleInputW, INPUT_RECORD,
        INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
        ShowWindow, SW_RESTORE,
    };

    /// Attaches to the session's console for the duration of `f`. Tokmira is a GUI app with
    /// no console of its own, so attaching is allowed.
    fn with_console<T>(pid: u32, f: impl FnOnce() -> T) -> Option<T> {
        // SAFETY: plain Win32 calls; the console is released before returning.
        unsafe {
            if AttachConsole(pid) == 0 {
                return None;
            }
            let result = f();
            FreeConsole();
            Some(result)
        }
    }

    /// Key-down and key-up events for each character; no Enter is sent.
    pub fn write_console_input(pid: u32, text: &str) -> bool {
        with_console(pid, || {
            let name: Vec<u16> = "CONIN$\0".encode_utf16().collect();
            // SAFETY: `name` is NUL-terminated; the handle is closed below.
            let input = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            if input == INVALID_HANDLE_VALUE {
                return false;
            }
            let records: Vec<INPUT_RECORD> = text
                .encode_utf16()
                .flat_map(|unit| {
                    [1, 0].map(|down| INPUT_RECORD {
                        EventType: KEY_EVENT as u16,
                        Event: INPUT_RECORD_0 {
                            KeyEvent: KEY_EVENT_RECORD {
                                bKeyDown: down,
                                wRepeatCount: 1,
                                wVirtualKeyCode: 0,
                                wVirtualScanCode: 0,
                                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
                                dwControlKeyState: 0,
                            },
                        },
                    })
                })
                .collect();
            let mut written = 0u32;
            // SAFETY: `records` outlives the call; `written` receives the count.
            let ok = unsafe {
                WriteConsoleInputW(
                    input,
                    records.as_ptr(),
                    u32::try_from(records.len()).unwrap_or(0),
                    &mut written,
                )
            };
            // SAFETY: `input` came from CreateFileW above.
            unsafe { CloseHandle(input) };
            ok != 0 && written as usize == records.len()
        })
        .unwrap_or(false)
    }

    fn bring_forward(window: HWND) -> bool {
        // SAFETY: `window` is a handle Windows just gave us.
        unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            }
            SetForegroundWindow(window) != 0
        }
    }

    /// Classic consoles have a real window; under Windows Terminal it is hidden.
    pub fn focus_console(pid: u32) -> bool {
        with_console(pid, || {
            // SAFETY: no arguments; valid while attached.
            let window = unsafe { GetConsoleWindow() };
            // SAFETY: checked for null first.
            !window.is_null() && unsafe { IsWindowVisible(window) } != 0 && bring_forward(window)
        })
        .unwrap_or(false)
    }

    struct Search {
        pid: u32,
        window: HWND,
    }

    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Search` passed to EnumWindows below.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: `owner` receives the process id.
        unsafe { GetWindowThreadProcessId(window, &mut owner) };
        // SAFETY: `window` comes from EnumWindows.
        if owner == search.pid && unsafe { IsWindowVisible(window) } != 0 {
            search.window = window;
            return 0;
        }
        1
    }

    /// The first visible top-level window of a process, brought forward.
    pub fn focus_process_window(pid: u32) -> bool {
        let mut search = Search {
            pid,
            window: ptr::null_mut(),
        };
        // SAFETY: `search` outlives the enumeration.
        unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
        !search.window.is_null() && bring_forward(search.window)
    }
}

#[cfg(test)]
mod tests {
    use super::{applescript_string, shell_quote, ActionPayload};
    use token_audit_core::advice::AdviceAlert;
    use token_audit_core::model::{Source, Tool};
    use token_audit_core::session_open::SessionRef;

    #[test]
    fn alert_payload_survives_the_notification_round_trip() {
        let session = SessionRef {
            tool: Tool::Claude,
            source: Source::ClaudeCli,
            session_id: "0a7ae72b-6b28".to_owned(),
            project: Some("/Users/me/project".to_owned()),
        };
        let alert = AdviceAlert {
            key: "claude:long_context".to_owned(),
            tool: Tool::Claude,
            session_id: session.session_id.clone(),
            title: "t".to_owned(),
            body: "b".to_owned(),
            session,
            command: Some("/compact".to_owned()),
        };
        let payload = ActionPayload::for_alert(&alert);
        let json = serde_json::to_string(&payload).expect("serialize");
        assert!(json.contains(r#""sessionId":"0a7ae72b-6b28""#));
        assert!(json.contains(r#""source":"ClaudeCli""#));
        let back: ActionPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, payload);
        assert_eq!(back.command.as_deref(), Some("/compact"));
    }

    #[test]
    fn quoting_survives_quotes_in_paths() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
