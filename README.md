# token-audit

本地运行的 AI 编程工具 Token 用量审计工具。命令行版可扫描 Claude Code 与 Codex 的本地日志；桌面界面目前仍是占位页面。扫描不会发起网络请求。

## 开发

需要 Rust stable、Node 22 和 pnpm。先在仓库根目录运行 `pnpm install`，再运行：

- 浏览器预览：`pnpm -C ui dev`
- 桌面应用：`pnpm tauri dev`
- 命令行版本：`cargo run -p token-audit-cli -- --version`

## scan 用法

运行 `cargo run -p token-audit-cli -- scan` 扫描当前用户目录下的 Claude Code 和 Codex 日志，输出中文汇总表。可用 `--home <目录>` 指定日志所在主目录；用 `--since YYYY-MM-DD`、`--until YYYY-MM-DD` 筛选本机时区的日期；用 `--tool claude|codex` 只看一种工具；加 `--json` 输出完整报告。

价格表内置于程序。默认会读取系统用户配置目录下的 `token-audit/pricing.toml` 作为覆盖文件，也可用 `--pricing <文件>` 指定。覆盖文件按模型整条覆盖或新增，未列出的模型沿用内置表。费用是按 API 价格的估算，不是实际账单；未定价事件会单独计数。

## Codex 官方额度

运行 `cargo run -p token-audit-cli -- quota codex`，通过已安装的 Codex 本地 `app-server` 读取当前账号的官方额度和按日 token 用量。可用 `--codex <路径>` 指定 Codex 可执行文件、`--timeout <秒>` 调整整体超时（默认 15 秒）、`--json` 输出结构化结果。此命令会启动 Codex，由 Codex 使用已登录账号查询 OpenAI；token-audit 不读取令牌。

## Claude Code 记录器

默认不安装。先用 `token-audit recorder install --dry-run` 查看计划，再用 `token-audit recorder install` 安装状态栏记录器；加 `--hooks` 可单独安装 hooks，加 `--statusline --hooks` 可同时安装。`token-audit recorder status` 查看安装状态，`token-audit recorder records` 查看最近的额度记录。管理命令支持 `--home <目录>` 和 `--data-dir <目录>`，`status` 和 `records` 支持 `--json`。

安装会修改 `<home>/.claude/settings.json`：状态栏替换为记录器命令并链式调用原命令；四类 hooks 只追加条目。每次修改前把原文件完整备份到 `<data_dir>/recorder/backups/`。用 `token-audit recorder uninstall` 先查看计划并卸载；可加 `--dry-run` 只预览。卸载保留记录文件；界面可通过核心库 `clear_records` 清除。默认数据目录为系统用户数据目录下的 `token-audit`。指定自定义 `--data-dir` 安装时，配置里的记录器命令会带上该路径，确保 Claude Code 调用时写入同一目录。

## 检查

依次运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --workspace`、`pnpm typecheck`、`pnpm build`。
