# token-audit

本地运行的 AI 编程工具 Token 用量审计工具。命令行版可扫描 Claude Code 与 Codex 的本地日志；桌面界面目前仍是占位页面。扫描不会发起网络请求。

## 开发

需要 Rust stable、Node 22 和 pnpm。先在仓库根目录运行 `pnpm install`，再运行：

- 浏览器预览：`pnpm -C ui dev`
- 桌面应用：`pnpm tauri dev`
- 命令行版本：`cargo run -p token-audit-cli -- --version`

仪表盘刻度与提示标签的纯函数测试：`pnpm test`（需要 Node 22）。

## scan 用法

运行 `cargo run -p token-audit-cli -- scan` 扫描当前用户目录下的 Claude Code 和 Codex 日志，输出中文汇总表。可用 `--home <目录>` 指定日志所在主目录；用 `--since YYYY-MM-DD`、`--until YYYY-MM-DD` 筛选本机时区的日期；用 `--tool claude|codex` 只看一种工具；加 `--json` 输出完整报告。

价格表内置于程序。默认会读取系统用户配置目录下的 `token-audit/pricing.toml` 作为覆盖文件，也可用 `--pricing <文件>` 指定。覆盖文件按模型整条覆盖或新增，未列出的模型沿用内置表。费用是按 API 价格的估算，不是实际账单；未定价事件会单独计数。

## Codex 官方额度

运行 `cargo run -p token-audit-cli -- quota codex`，通过已安装的 Codex 本地 `app-server` 读取当前账号的官方额度和按日 token 用量。可用 `--codex <路径>` 指定 Codex 可执行文件、`--timeout <秒>` 调整整体超时（默认 15 秒）、`--json` 输出结构化结果。此命令会启动 Codex，由 Codex 使用已登录账号查询 OpenAI；token-audit 不读取令牌。

## Claude 官方额度

手动运行 `cargo run -p token-audit-cli -- quota claude` 才会查询。可加 `--source auto|desktop|code` 选择凭据（默认先桌面版再 Claude Code）、`--proxy http://host:port`、`--no-proxy`、`--timeout <秒>`（默认 15 秒）、`--keychain-timeout <秒>`（桌面版钥匙串授权默认等待 60 秒）、`--json`。macOS 上读取 Claude Code 凭据使用系统 `security` 命令，最多等待 10 秒。代理优先级为命令行参数、`HTTPS_PROXY` / `https_proxy` / `ALL_PROXY` 环境变量、系统静态代理；`--no-proxy` 禁用代理。macOS 的 PAC 自动配置代理暂不支持。

此命令会短暂读取本机 Claude 登录令牌，只向 `https://api.anthropic.com/api/oauth/usage` 发送查询；不会保存、打印或刷新令牌，也不会跟随重定向。令牌过期时请先打开对应 Claude 应用，由它自行刷新。Windows 当前只支持 Claude Code 凭据，Claude 桌面版的 DPAPI 格式尚未验证。接口未公开，成功响应结构尚未实测，后续可能变化。普通日志扫描和记录器不会执行此查询。

## Claude Code 记录器

默认不安装。先用 `token-audit recorder install --dry-run` 查看计划，再用 `token-audit recorder install` 安装状态栏记录器；加 `--hooks` 可单独安装 hooks，加 `--statusline --hooks` 可同时安装。`token-audit recorder status` 查看安装状态，`token-audit recorder records` 查看最近的额度记录。管理命令支持 `--home <目录>` 和 `--data-dir <目录>`，`status` 和 `records` 支持 `--json`。

安装会修改 `<home>/.claude/settings.json`：状态栏替换为记录器命令并链式调用原命令；四类 hooks 只追加条目。每次修改前把原文件完整备份到 `<data_dir>/recorder/backups/`。用 `token-audit recorder uninstall` 先查看计划并卸载；可加 `--dry-run` 只预览。卸载保留记录文件；界面可通过核心库 `clear_records` 清除。默认数据目录为系统用户数据目录下的 `token-audit`。指定自定义 `--data-dir` 安装时，配置里的记录器命令会带上该路径，确保 Claude Code 调用时写入同一目录。

## 检查

依次运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo build --workspace`、`pnpm typecheck`、`pnpm build`。
