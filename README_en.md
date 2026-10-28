<div align="center">

<img src="src-tauri/icons/128x128@2x.png" alt="token-audit" width="88">

# token-audit

See where your Claude Code and Codex quota goes, and why it runs out. Your data stays on your machine.

[简体中文](README.md) · **English**

[![Release](https://img.shields.io/github/v/release/WenWangxin/token-audit?style=flat-square&label=release)](https://github.com/WenWangxin/token-audit/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/WenWangxin/token-audit/total?style=flat-square&logo=github)](https://github.com/WenWangxin/token-audit/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey?style=flat-square)](#download)

[Website](https://wenwangxin.github.io/token-audit/en/) · [Download](#download) · [Changelog](https://github.com/WenWangxin/token-audit/releases) · [Privacy](#privacy) · [Build from source](#build-from-source)

</div>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="snapshots/preview-dark.png">
    <img alt="token-audit dashboard: 5-hour and weekly quota for Claude and Codex, today's usage, the quota trend and saving advice" src="snapshots/preview.png" width="92%">
  </picture>
</p>

> The screenshots use the built-in demo data. The interface is available in English and Chinese.

## Features

- **Quota at a glance**: 5-hour and weekly quota for Claude and Codex, reset times and a forecast of when you hit the limit. Between two official readings, the current quota is estimated from new local usage and marked with ≈.
- **Where the quota went**: usage split by session, project, model, subagent and single prompt, so you can see which prompt used it up.
- **Saving advice**: 16 kinds of waste found in your local logs, such as a cache rebuilt after it expired, an overlong context, a light task on an expensive model, or a prompt that ran far too many turns, each with what it cost and how to avoid it next time.
- **Background reminders**: with the window closed, the app keeps running in the menu bar or tray and notifies you as soon as a costly pattern shows up.
- **Quota in the menu bar**: the 5-hour quota on the macOS menu bar, or in the Windows tray tooltip, without opening the window.
- **Sessions and timelines**: each session's requests, prompts and diagnosis tags laid out in time.
- **Is the subscription worth it**: this month's usage at API prices, as a card you can share.
- **English and Chinese interface**, launch at login, silent start and optional in-app updates.

> Since v1.2.0, diagnosis text, session tags and background advice notifications follow the selected language. The CLI continues to output Chinese by default.

## Download

Get the latest version from [GitHub Releases](https://github.com/WenWangxin/token-audit/releases/latest).

| Platform | Package | Notes |
| --- | --- | --- |
| macOS | `token-audit-<version>-macos-universal.dmg` | Apple silicon and Intel |
| Windows 10 / 11 (x64) | `token-audit-<version>-windows-amd64-setup.exe` | Most PCs |
| Windows 10 / 11 (ARM64) | `token-audit-<version>-windows-arm64-setup.exe` | Snapdragon and other ARM laptops |

The packages are not code-signed or notarized yet:

- **macOS**: the first time, right-click the app in Finder and choose Open. If it is still blocked, allow it in System Settings → Privacy & Security.
- **Windows**: if SmartScreen appears, choose More info → Run anyway.

**Supported tools**: Claude Code in the terminal and in the desktop app, and Codex in the terminal and in the desktop app. Regular chats in the Claude desktop app or on the web, and ChatGPT chats, leave no local usage records and are not counted; the part of the official quota that local logs cannot explain is shown as "unattributed".

## Privacy

token-audit is only useful if you can trust it, so three principles do not change:

1. **Your data stays on your machine by default.** Usage analysis selects usage and event metadata from local logs without uploading conversation content. A login token is used briefly in memory only if you enable the Claude official quota query, and is sent only to Anthropic.
2. **The core logic is open source.** The counting, attribution and diagnosis code is all in this repository, so you can reproduce every result.
3. **Results cannot be bought.** Nobody can pay to change a diagnosis.

Official quota queries and update checks require you to enable or initiate them:

- **Claude official quota query**: reads your local Claude login token and sends it only to Anthropic. The token is kept in memory only, never saved or refreshed.
- **Codex official quota query**: the local Codex interface queries OpenAI using your signed-in account. token-audit does not read the login token, and local log analysis works independently.
- **Checking for updates**: downloads the update manifest and packages only from this repository's GitHub Releases, without your version, device details or any usage data.

The optional Claude Code recorder (status line and hooks) records only quota percentages and prompt times, never prompt content. Before installing, it lists the settings it will change and backs up the original file; uninstalling restores it.

## Usage

1. Open the app after installing. The first-run guide detects Claude Code and Codex on your machine and explains each data source.
2. Codex quota comes straight from its local logs; nothing to set up.
3. For exact Claude quota, choose one: turn on the Claude official quota query in Settings, or install the status line recorder (terminal Claude Code only). If you want neither, you can calibrate by hand in Settings.

**The command-line tool** ships with the app: on macOS at `token-audit.app/Contents/MacOS/token-audit`, on Windows as `token-audit.exe` in the install folder. Common commands:

```bash
token-audit scan --since 2026-10-01   # usage summary for local Claude Code and Codex
token-audit advice --days 7           # saving advice for the last 7 days
token-audit sessions --days 7         # session list
token-audit quota codex               # official quota through Codex's local app-server
```

Every command supports `--json`. Run `token-audit` without arguments for the full usage. Its output is in Chinese for now.

## Build from source

You need [Rust](https://rustup.rs/) stable, [Node.js](https://nodejs.org/) 22 and [pnpm](https://pnpm.io/).

```bash
git clone https://github.com/WenWangxin/token-audit.git
cd token-audit
pnpm install
cargo build -p token-audit-cli   # the development desktop app calls it
pnpm tauri dev                   # start the desktop app
```

| Target | Command | Notes |
| --- | --- | --- |
| macOS universal | `pnpm bundle:mac` | Builds the command-line tool for both architectures, merges them, then packages the `.app` and `.dmg` |
| Windows | `.\scripts\build-sidecar.ps1 -Target x86_64-pc-windows-msvc`, then `pnpm tauri build --target x86_64-pc-windows-msvc --bundles nsis` | Use `aarch64-pc-windows-msvc` for ARM64 |

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml) when a `v*` tag is pushed. Bundling with in-app updates needs the update signing key; for your own local builds, set `bundle.createUpdaterArtifacts` to `false` in `src-tauri/tauri.conf.json`.

Checks:

```bash
cargo test --workspace
pnpm -C ui typecheck
pnpm -C ui test
```

Prices and diagnosis thresholds live in [`config/`](config). Costs are estimates at API prices, not actual bills.

## Feedback

Please report bugs and ideas in [GitHub Issues](https://github.com/WenWangxin/token-audit/issues). If you find it useful, a star is the easiest way to support it.

## License

token-audit is released under the [MIT License](LICENSE).
