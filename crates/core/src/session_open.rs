//! Where a coding session runs and how to get back to it: the terminal a CLI session lives
//! in, the desktop app's link for an app session, and the command that resumes an ended one.
//! Pure decisions only; the desktop app does the driving.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::model::{Source, Tool, UsageEvent};

/// The only environment variables ever read from a session's process or a hook. A process
/// environment also holds API keys, so nothing outside this list is looked at or kept.
pub const TERMINAL_ENV_KEYS: [&str; 13] = [
    "TERM_PROGRAM",
    "__CFBundleIdentifier",
    "ITERM_SESSION_ID",
    "TERM_SESSION_ID",
    "TMUX",
    "TMUX_PANE",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "KITTY_WINDOW_ID",
    "KITTY_LISTEN_ON",
    "WT_SESSION",
    "TERMINAL_EMULATOR",
    "VSCODE_GIT_IPC_HANDLE",
];

/// Keys that name one tab or pane; two processes sharing any of them run in the same place.
const PLACE_KEYS: [&str; 6] = [
    "ITERM_SESSION_ID",
    "TERM_SESSION_ID",
    "TMUX_PANE",
    "WEZTERM_PANE",
    "KITTY_WINDOW_ID",
    "WT_SESSION",
];

const VALUE_LIMIT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    ITerm,
    Ghostty,
    AppleTerminal,
    WezTerm,
    Kitty,
    Warp,
    Alacritty,
    VsCode,
    Cursor,
    Windsurf,
    Zed,
    JetBrains,
    WindowsTerminal,
    Unknown,
}

/// The allowlisted terminal variables of one process.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TerminalEnv(BTreeMap<String, String>);

impl TerminalEnv {
    /// Keeps only [`TERMINAL_ENV_KEYS`]; values with control characters or over 512 bytes are
    /// dropped rather than trimmed.
    pub fn capture(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut values = BTreeMap::new();
        for key in TERMINAL_ENV_KEYS {
            let Some(value) = get(key) else { continue };
            let value = value.trim();
            if value.is_empty() || value.len() > VALUE_LIMIT || value.chars().any(char::is_control)
            {
                continue;
            }
            values.insert(key.to_owned(), value.to_owned());
        }
        Self(values)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The app that hosts the session. A tmux pane reports the terminal it runs inside.
    #[must_use]
    pub fn kind(&self) -> TerminalKind {
        if let Some(kind) = self.bundle_id().and_then(kind_from_bundle) {
            return kind;
        }
        if self.get("WT_SESSION").is_some() {
            return TerminalKind::WindowsTerminal;
        }
        if self.get("KITTY_WINDOW_ID").is_some() {
            return TerminalKind::Kitty;
        }
        if self
            .get("TERMINAL_EMULATOR")
            .is_some_and(|value| value.starts_with("JetBrains"))
        {
            return TerminalKind::JetBrains;
        }
        match self.get("TERM_PROGRAM") {
            Some("iTerm.app") => TerminalKind::ITerm,
            Some("ghostty") => TerminalKind::Ghostty,
            Some("Apple_Terminal") => TerminalKind::AppleTerminal,
            Some("WezTerm") => TerminalKind::WezTerm,
            Some("WarpTerminal") => TerminalKind::Warp,
            Some("vscode") => TerminalKind::VsCode,
            Some("zed") => TerminalKind::Zed,
            _ if self.get("WEZTERM_PANE").is_some() => TerminalKind::WezTerm,
            _ => TerminalKind::Unknown,
        }
    }

    /// The macOS app bundle that started the shell, when it looks like a bundle identifier.
    #[must_use]
    pub fn bundle_id(&self) -> Option<&str> {
        self.get("__CFBundleIdentifier").filter(|id| {
            id.contains('.')
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        })
    }

    /// `w0t1p0:UUID` → `UUID`, the id iTerm2's AppleScript knows a session by.
    #[must_use]
    pub fn iterm_session(&self) -> Option<&str> {
        let value = self.get("ITERM_SESSION_ID")?;
        let id = value.rsplit(':').next()?;
        is_safe_id(id).then_some(id)
    }

    /// The tmux server socket and pane (`%3`) the session runs in.
    #[must_use]
    pub fn tmux(&self) -> Option<(&str, &str)> {
        let socket = self.get("TMUX")?.split(',').next()?;
        let pane = self.get("TMUX_PANE")?;
        let valid = pane.len() > 1
            && pane.starts_with('%')
            && pane[1..].chars().all(|ch| ch.is_ascii_digit());
        (valid && !socket.is_empty()).then_some((socket, pane))
    }

    #[must_use]
    pub fn wezterm_pane(&self) -> Option<u64> {
        self.get("WEZTERM_PANE")?.parse().ok()
    }

    #[must_use]
    pub fn kitty_window(&self) -> Option<u64> {
        self.get("KITTY_WINDOW_ID")?.parse().ok()
    }

    /// True when it names a tab or pane, not just the terminal app.
    #[must_use]
    pub fn has_place(&self) -> bool {
        PLACE_KEYS.iter().any(|key| self.get(key).is_some())
    }

    /// True when both name the same tab or pane.
    #[must_use]
    pub fn same_place(&self, other: &Self) -> bool {
        PLACE_KEYS
            .iter()
            .any(|key| self.get(key).is_some() && self.get(key) == other.get(key))
    }
}

fn kind_from_bundle(id: &str) -> Option<TerminalKind> {
    Some(match id {
        "com.googlecode.iterm2" => TerminalKind::ITerm,
        "com.mitchellh.ghostty" => TerminalKind::Ghostty,
        "com.apple.Terminal" => TerminalKind::AppleTerminal,
        "com.github.wez.wezterm" => TerminalKind::WezTerm,
        "net.kovidgoyal.kitty" => TerminalKind::Kitty,
        "org.alacritty" => TerminalKind::Alacritty,
        "com.microsoft.VSCode" | "com.microsoft.VSCodeInsiders" => TerminalKind::VsCode,
        "com.todesktop.230313mzl4w4u92" => TerminalKind::Cursor,
        "com.exafunction.windsurf" => TerminalKind::Windsurf,
        "dev.zed.Zed" | "dev.zed.Zed-Preview" => TerminalKind::Zed,
        id if id.starts_with("dev.warp.") => TerminalKind::Warp,
        id if id.starts_with("com.jetbrains.") || id.starts_with("com.google.android.studio") => {
            TerminalKind::JetBrains
        }
        _ => return None,
    })
}

/// Ids that go into links, shell commands and scripts: letters, digits, `-` and `_` only.
#[must_use]
pub fn is_safe_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// Slash commands Tokmira may type for the user, such as `/compact` or `/model sonnet`.
#[must_use]
pub fn is_fill_command(command: &str) -> bool {
    command.len() <= 64
        && command.starts_with('/')
        && command.chars().all(|ch| {
            ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || matches!(ch, ' ' | '/' | '-' | '_' | '.')
        })
}

/// The shell command that reopens an ended CLI session.
#[must_use]
pub fn resume_command(tool: Tool, session_id: &str) -> Option<String> {
    if !is_safe_id(session_id) {
        return None;
    }
    Some(match tool {
        Tool::Claude => format!("claude --resume {session_id}"),
        Tool::Codex => format!("codex resume {session_id}"),
    })
}

/// Opens a Claude desktop Code session (`local_…` id).
#[must_use]
pub fn claude_desktop_link(local_id: &str) -> Option<String> {
    let valid = local_id
        .strip_prefix("local_")
        .is_some_and(|rest| (1..=64).contains(&rest.len()) && is_safe_id(rest));
    valid.then(|| format!("claude://code/continue?session={local_id}"))
}

/// Opens a Codex desktop thread; the thread id is the session id in Codex's logs.
#[must_use]
pub fn codex_desktop_link(thread_id: &str) -> Option<String> {
    is_safe_id(thread_id).then(|| format!("codex://threads/{thread_id}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopSessionFile {
    session_id: String,
    cli_session_id: Option<String>,
    #[serde(default)]
    prior_cli_session_ids: Vec<String>,
}

/// Finds the Claude desktop session (`local_…`) that ran a Claude Code session. Reads only
/// `<dir>/<account>/<org>/local_*.json`, two levels down, and only the three id fields.
#[must_use]
pub fn claude_desktop_session(sessions_dir: &Path, cli_session_id: &str) -> Option<String> {
    if !is_safe_id(cli_session_id) {
        return None;
    }
    for account in fs::read_dir(sessions_dir).ok()?.flatten() {
        let Ok(orgs) = fs::read_dir(account.path()) else {
            continue;
        };
        for org in orgs.flatten() {
            let Ok(files) = fs::read_dir(org.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with("local_") || !name.ends_with(".json") {
                    continue;
                }
                let Ok(bytes) = fs::read(file.path()) else {
                    continue;
                };
                let Ok(session) = serde_json::from_slice::<DesktopSessionFile>(&bytes) else {
                    continue;
                };
                let matches = session.cli_session_id.as_deref() == Some(cli_session_id)
                    || session
                        .prior_cli_session_ids
                        .iter()
                        .any(|id| id == cli_session_id);
                if matches && claude_desktop_link(&session.session_id).is_some() {
                    return Some(session.session_id);
                }
            }
        }
    }
    None
}

/// True for an interactive Claude Code or Codex CLI process. Desktop apps run the same
/// programs headless (`stream-json`, `app-server`), and those are left out.
#[must_use]
pub fn is_cli_process(tool: Tool, exe: &str, args: &[String]) -> bool {
    // Split on both separators so a Windows path reads the same on every host.
    let base = |path: &str| {
        let name = path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
    };
    // The native Claude Code installer runs `~/.local/bin/claude`, a link to a file named
    // after its version, so the program name comes from argv[0] as well.
    let names = [Some(base(exe)), args.first().map(|arg| base(arg))];
    let named = |wanted: &str| names.iter().flatten().any(|name| name == wanted);
    let from_app = exe.contains(".app/") || exe.contains("Application Support");
    let has = |needle: &str| args.iter().any(|arg| arg.contains(needle));
    match tool {
        Tool::Claude => {
            // Older npm installs run `node <prefix>/bin/claude` or the package's `cli.js`.
            let node_launcher = named("node")
                && (has("@anthropic-ai/claude-code")
                    || has("claude-code/cli")
                    || args.get(1).is_some_and(|arg| base(arg) == "claude"));
            let cli = named("claude") || node_launcher;
            cli && !from_app && !has("stream-json")
        }
        Tool::Codex => {
            const HEADLESS: [&str; 5] = ["app-server", "exec-server", "exec", "mcp", "mcp-server"];
            // Any position: options such as `-c key=value` can come before the subcommand.
            named("codex")
                && !from_app
                && !args
                    .iter()
                    .skip(1)
                    .any(|arg| HEADLESS.contains(&arg.as_str()))
        }
    }
}

/// A running CLI process in the session's folder.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub mentions_session: bool,
    pub env: TerminalEnv,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Known to be this session: its command line names it, or it runs where a hook saw it.
    Exact(usize),
    /// The only CLI process in the folder; most likely this session.
    Only(usize),
    Ambiguous,
    None,
}

#[must_use]
pub fn pick_process(candidates: &[Candidate], recorded: Option<&TerminalEnv>) -> Pick {
    if let Some(index) = candidates.iter().position(|item| item.mentions_session) {
        return Pick::Exact(index);
    }
    if let Some(recorded) = recorded {
        let places: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, item)| item.env.same_place(recorded))
            .map(|(index, _)| index)
            .collect();
        if let [index] = places[..] {
            return Pick::Exact(index);
        }
        // The session's own tab is known and no process runs there: whatever runs in the
        // folder now is another session, and this one has ended.
        if places.is_empty() && recorded.has_place() {
            return Pick::None;
        }
    }
    match candidates.len() {
        0 => Pick::None,
        1 => Pick::Only(0),
        _ => Pick::Ambiguous,
    }
}

/// Whether Tokmira can type into this terminal without pressing Enter.
#[must_use]
pub fn can_fill(env: &TerminalEnv) -> bool {
    if env.tmux().is_some() {
        return true;
    }
    match env.kind() {
        TerminalKind::ITerm => env.iterm_session().is_some(),
        TerminalKind::WezTerm => env.wezterm_pane().is_some(),
        TerminalKind::Kitty => env.kitty_window().is_some(),
        TerminalKind::Ghostty | TerminalKind::WindowsTerminal => true,
        _ => cfg!(windows),
    }
}

/// Enough to find a session again: which program ran it, where, and its id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionRef {
    pub tool: Tool,
    pub source: Source,
    pub session_id: String,
    pub project: Option<String>,
}

impl SessionRef {
    #[must_use]
    pub fn of(event: &UsageEvent) -> Self {
        Self {
            tool: event.tool,
            source: event.source,
            session_id: event.session_id.clone(),
            project: event.project.clone(),
        }
    }
}

/// What happened when the user asked to open a session or fill a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenOutcome {
    /// The session's tab is in front.
    Focused,
    /// The tab is in front and the command is typed, waiting for Enter.
    Filled,
    /// The tab is in front; the command is on the clipboard.
    FocusedCopied,
    /// The app is in front (the exact tab could not be told apart); the command is copied.
    AppCopied,
    /// The app is in front; nothing to fill.
    AppOpened,
    /// The session had ended and was reopened in a new window.
    Resumed,
    /// The session could not be found or reopened; the command, if any, is copied.
    NotFound,
}

/// The follow-up notice after a notification button; `None` when the result is visible on
/// screen and the clipboard was left alone.
#[must_use]
pub fn outcome_notice(outcome: OpenOutcome, command: Option<&str>, lang: Lang) -> Option<String> {
    let copied = |zh: &str, en: &str| {
        command.map(|command| match lang {
            Lang::Zh => format!("{zh}{command} 已复制，粘贴后发送。"),
            Lang::En => format!("{en}{command} is copied; paste it to send."),
        })
    };
    match outcome {
        OpenOutcome::Filled | OpenOutcome::Focused => None,
        OpenOutcome::Resumed => copied(
            "这个会话已结束，已在新窗口中重新打开。",
            "This session had ended, so it was reopened in a new window. ",
        ),
        OpenOutcome::FocusedCopied => copied("已切换到这个会话。", "Switched to this session. "),
        OpenOutcome::AppCopied | OpenOutcome::AppOpened => Some(
            copied(
                "已打开会话所在的应用，但分不清是哪个标签页。",
                "Opened the app this session runs in, but couldn’t tell which tab. ",
            )
            .unwrap_or_else(|| {
                lang.pick(
                    "已打开会话所在的应用，但分不清是哪个标签页。",
                    "Opened the app this session runs in, but couldn’t tell which tab.",
                )
                .to_owned()
            }),
        ),
        OpenOutcome::NotFound => Some(
            copied(
                "找不到这个会话所在的终端或应用。",
                "Couldn’t find the terminal or app this session runs in. ",
            )
            .unwrap_or_else(|| {
                lang.pick(
                    "找不到这个会话所在的终端或应用。",
                    "Couldn’t find the terminal or app this session runs in.",
                )
                .to_owned()
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> TerminalEnv {
        TerminalEnv::capture(|key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn capture_keeps_only_the_allowlist() {
        let captured = env(&[
            ("TERM_PROGRAM", "iTerm.app"),
            ("ANTHROPIC_API_KEY", "sk-secret"),
            ("ITERM_SESSION_ID", "w0t1p0:ABCD-1234"),
            ("TMUX_PANE", "bad\nvalue"),
        ]);
        assert_eq!(captured.get("TERM_PROGRAM"), Some("iTerm.app"));
        assert_eq!(captured.get("ANTHROPIC_API_KEY"), None);
        assert_eq!(captured.get("TMUX_PANE"), None);
        assert_eq!(captured.iterm_session(), Some("ABCD-1234"));
    }

    #[test]
    fn kind_prefers_the_hosting_app_over_tmux() {
        let inside_tmux = env(&[
            ("TERM_PROGRAM", "tmux"),
            ("__CFBundleIdentifier", "com.mitchellh.ghostty"),
            ("TMUX", "/private/tmp/tmux-501/default,123,0"),
            ("TMUX_PANE", "%4"),
        ]);
        assert_eq!(inside_tmux.kind(), TerminalKind::Ghostty);
        assert_eq!(
            inside_tmux.tmux(),
            Some(("/private/tmp/tmux-501/default", "%4"))
        );
        assert!(can_fill(&inside_tmux));

        let cursor = env(&[
            ("TERM_PROGRAM", "vscode"),
            ("__CFBundleIdentifier", "com.todesktop.230313mzl4w4u92"),
        ]);
        assert_eq!(cursor.kind(), TerminalKind::Cursor);
        assert_eq!(
            env(&[("TERM_PROGRAM", "vscode")]).kind(),
            TerminalKind::VsCode
        );
        assert_eq!(
            env(&[("WT_SESSION", "guid")]).kind(),
            TerminalKind::WindowsTerminal
        );
        assert_eq!(
            env(&[("TERMINAL_EMULATOR", "JetBrains-JediTerm")]).kind(),
            TerminalKind::JetBrains
        );
        assert_eq!(
            env(&[("TERM_PROGRAM", "Apple_Terminal")]).kind(),
            TerminalKind::AppleTerminal
        );
    }

    #[test]
    fn tmux_pane_must_look_like_a_pane() {
        assert_eq!(
            env(&[("TMUX", "/tmp/s,1,0"), ("TMUX_PANE", "%x")]).tmux(),
            None
        );
        assert_eq!(env(&[("TMUX_PANE", "%1")]).tmux(), None);
    }

    #[test]
    fn ids_and_commands_are_checked_before_use() {
        assert!(is_safe_id("0a7ae72b-6b28-4524-b343-c3e484ef768b"));
        assert!(!is_safe_id("abc; rm -rf ~"));
        assert!(!is_safe_id(""));
        assert!(is_fill_command("/compact"));
        assert!(is_fill_command("/model sonnet"));
        assert!(!is_fill_command("compact"));
        assert!(!is_fill_command("/compact\n"));
        assert!(!is_fill_command("/x; echo"));
        assert_eq!(
            resume_command(Tool::Claude, "abc-1"),
            Some("claude --resume abc-1".to_owned())
        );
        assert_eq!(
            resume_command(Tool::Codex, "abc-1"),
            Some("codex resume abc-1".to_owned())
        );
        assert_eq!(resume_command(Tool::Codex, "a b"), None);
    }

    #[test]
    fn desktop_links_only_take_valid_ids() {
        assert_eq!(
            claude_desktop_link("local_1862e8d0-9f4b"),
            Some("claude://code/continue?session=local_1862e8d0-9f4b".to_owned())
        );
        assert_eq!(claude_desktop_link("1862e8d0"), None);
        assert_eq!(claude_desktop_link("local_a&b=c"), None);
        assert_eq!(
            codex_desktop_link("01a180aa-73fd"),
            Some("codex://threads/01a180aa-73fd".to_owned())
        );
        assert_eq!(codex_desktop_link("../x"), None);
    }

    #[test]
    fn desktop_session_is_found_by_current_or_earlier_cli_id() {
        let root = std::env::temp_dir().join(format!("tokmira-desktop-{}", std::process::id()));
        let org = root.join("account").join("org");
        fs::create_dir_all(&org).unwrap();
        fs::write(
            org.join("local_one.json"),
            r#"{"sessionId":"local_one","cliSessionId":"cli-new","priorCliSessionIds":["cli-old"],"title":"x"}"#,
        )
        .unwrap();
        fs::write(
            org.join("notes.json"),
            r#"{"sessionId":"local_two","cliSessionId":"cli-x"}"#,
        )
        .unwrap();
        assert_eq!(
            claude_desktop_session(&root, "cli-new"),
            Some("local_one".to_owned())
        );
        assert_eq!(
            claude_desktop_session(&root, "cli-old"),
            Some("local_one".to_owned())
        );
        assert_eq!(claude_desktop_session(&root, "cli-x"), None);
        assert_eq!(claude_desktop_session(&root, "missing"), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cli_processes_exclude_desktop_and_headless_runs() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert!(is_cli_process(
            Tool::Claude,
            "/Users/me/.local/bin/claude",
            &args(&["claude"])
        ));
        assert!(is_cli_process(
            Tool::Claude,
            "/Users/me/.local/share/claude/versions/2.1.3",
            &args(&["claude", "--resume", "abc"])
        ));
        assert!(is_cli_process(
            Tool::Claude,
            "/usr/local/bin/node",
            &args(&[
                "node",
                "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js"
            ])
        ));
        assert!(!is_cli_process(
            Tool::Claude,
            "/Users/me/Library/Application Support/Claude/claude-code/2.1/claude",
            &args(&["claude", "--output-format", "stream-json"])
        ));
        assert!(is_cli_process(
            Tool::Codex,
            "/opt/homebrew/bin/codex",
            &args(&["codex"])
        ));
        assert!(is_cli_process(
            Tool::Codex,
            "/opt/homebrew/bin/codex",
            &args(&["codex", "resume", "abc"])
        ));
        assert!(!is_cli_process(
            Tool::Codex,
            "/opt/homebrew/bin/codex",
            &args(&["codex", "app-server"])
        ));
        assert!(is_cli_process(
            Tool::Claude,
            "/usr/local/bin/node",
            &args(&["node", "/usr/local/bin/claude"])
        ));
        assert!(!is_cli_process(
            Tool::Codex,
            "/opt/homebrew/bin/codex",
            &args(&["codex", "-c", "features.x=true", "app-server"])
        ));
        assert!(!is_cli_process(
            Tool::Codex,
            "/Applications/ChatGPT.app/Contents/Resources/codex-cli/codex",
            &args(&["codex"])
        ));
        assert!(is_cli_process(
            Tool::Codex,
            "C:\\bin\\codex.exe",
            &args(&["codex.exe"])
        ));
    }

    #[test]
    fn notices_mention_the_clipboard_only_when_it_changed() {
        assert_eq!(
            outcome_notice(OpenOutcome::Filled, Some("/compact"), Lang::Zh),
            None
        );
        assert_eq!(outcome_notice(OpenOutcome::Resumed, None, Lang::Zh), None);
        assert_eq!(
            outcome_notice(OpenOutcome::Resumed, Some("/compact"), Lang::Zh).as_deref(),
            Some("这个会话已结束，已在新窗口中重新打开。/compact 已复制，粘贴后发送。")
        );
        assert_eq!(
            outcome_notice(OpenOutcome::FocusedCopied, Some("/compact"), Lang::Zh).as_deref(),
            Some("已切换到这个会话。/compact 已复制，粘贴后发送。")
        );
        assert_eq!(
            outcome_notice(OpenOutcome::NotFound, None, Lang::En).as_deref(),
            Some("Couldn’t find the terminal or app this session runs in.")
        );
    }

    #[test]
    fn pick_prefers_the_named_session_then_the_recorded_tab() {
        let tab = |id: &str| Candidate {
            mentions_session: false,
            env: env(&[("ITERM_SESSION_ID", id)]),
        };
        let named = Candidate {
            mentions_session: true,
            env: TerminalEnv::default(),
        };
        assert_eq!(
            pick_process(&[tab("w0t0p0:A"), named.clone()], None),
            Pick::Exact(1)
        );
        let recorded = env(&[("ITERM_SESSION_ID", "w0t0p0:B")]);
        assert_eq!(
            pick_process(&[tab("w0t0p0:A"), tab("w0t0p0:B")], Some(&recorded)),
            Pick::Exact(1)
        );
        assert_eq!(
            pick_process(&[tab("w0t0p0:A"), tab("w0t0p0:C")], Some(&recorded)),
            Pick::None
        );
        assert_eq!(
            pick_process(&[tab("w0t0p0:A"), tab("w0t0p0:C")], None),
            Pick::Ambiguous
        );
        assert_eq!(pick_process(&[tab("w0t0p0:A")], None), Pick::Only(0));
        assert_eq!(
            pick_process(&[tab("w0t0p0:A")], Some(&recorded)),
            Pick::None
        );
        let app_only = env(&[("TERM_PROGRAM", "ghostty")]);
        assert_eq!(
            pick_process(&[tab("w0t0p0:A")], Some(&app_only)),
            Pick::Only(0)
        );
        assert_eq!(pick_process(&[], None), Pick::None);
    }
}
