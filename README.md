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

## 检查

依次运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --workspace`、`pnpm typecheck`、`pnpm build`。
