# token-audit

本地运行的 AI 编程工具 Token 用量审计工具。当前 M0 仅包含工程骨架与五个占位页面，不读取日志，也不发起网络请求。

## 开发

需要 Rust stable、Node 22 和 pnpm。先在仓库根目录运行 `pnpm install`，再运行：

- 浏览器预览：`pnpm -C ui dev`
- 桌面应用：`pnpm tauri dev`
- 命令行版本：`cargo run -p token-audit-cli -- --version`

## 检查

依次运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --workspace`、`pnpm typecheck`、`pnpm build`。
