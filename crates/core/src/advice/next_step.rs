//! One-line "next step" for each rule, with a copyable command only when verified.
//!
//! Commands live here rather than in `advice.toml` so an override file cannot add an
//! unverified command. Verified on 2026-10-29 against Claude Code 2.1.309: `/compact`,
//! `/model <alias>` and `/effort <level>`. `/fast` exists but toggles, so copying it could
//! turn fast mode on; it is left as text. No Codex command has been verified yet.

use serde::Serialize;

use super::AdviceConfig;
use crate::i18n::Lang;
use crate::model::Tool;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NextStep {
    pub text: String,
    /// Copyable command; `None` when the command for this tool is not verified.
    pub command: Option<String>,
}

fn command(tool: Tool, id: &str, config: &AdviceConfig) -> Option<&'static str> {
    match (tool, id) {
        (Tool::Claude, "long_context") => Some("/compact"),
        // The facts compare against the configured alternative, so only offer the
        // command when it switches to that model.
        (Tool::Claude, "expensive_light_task")
            if config
                .light_task
                .claude_alternative
                .to_ascii_lowercase()
                .contains("sonnet") =>
        {
            Some("/model sonnet")
        }
        // Only fires at xhigh / max, so `high` is a step down. `heavy_reasoning` does not
        // know the current level, where the same command could raise it; it stays text.
        (Tool::Claude, "high_effort_light_task") => Some("/effort high"),
        _ => None,
    }
}

fn text(tool: Tool, id: &str, lang: Lang) -> Option<&'static str> {
    let claude = tool == Tool::Claude;
    Some(match id {
        "cache_rebuild" => lang.pick(
            "离开超过缓存有效期后，开启新会话并只带必要上下文",
            "After a break longer than the cache lifetime, start a new session with only the context you need",
        ),
        "session_startup" if claude => lang.pick(
            "可检查不常用的 MCP 服务器和插件，连续的小任务放在同一会话",
            "Review MCP servers and plugins you rarely use, and keep related small tasks in one session",
        ),
        "session_startup" => lang.pick(
            "可检查不常用的 MCP 服务器和说明文件，连续的小任务放在同一会话",
            "Review MCP servers and instruction files you rarely use, and keep related small tasks in one session",
        ),
        "long_context" if claude => lang.pick(
            "任务阶段完成后在本会话执行 /compact，或开启新会话",
            "After a task stage, run /compact in this session or start a new one",
        ),
        "long_context" => lang.pick(
            "任务阶段完成后压缩上下文，或开启新会话",
            "After a task stage, compact the context or start a new session",
        ),
        "expensive_light_task" if claude => lang.pick(
            "做短小任务前先切换到 Sonnet",
            "Switch to Sonnet before short tasks",
        ),
        "expensive_light_task" => lang.pick(
            "做短小任务前在 Codex 中换用更便宜的模型",
            "Switch to a cheaper model in Codex before short tasks",
        ),
        "many_subagents" => lang.pick(
            "目标明确的小范围搜索直接在主对话中完成",
            "Run small, well-defined searches in the main session",
        ),
        "model_switch" => lang.pick(
            "换模型放在任务阶段之间，或换模型后开启新会话",
            "Switch models between task stages, or start a new session after switching",
        ),
        "context_spike" => lang.pick(
            "大文件和日志只读取相关部分，大 diff 分批查看",
            "Read only the relevant parts of large files and logs, and review large diffs in batches",
        ),
        "runaway_prompt" => lang.pick(
            "先写清完成条件；反复修同一个错误时打断并缩小范围",
            "State completion criteria first; interrupt and narrow the scope if the same error keeps coming back",
        ),
        "expensive_subagent" if claude => lang.pick(
            "在子代理定义里给简单子任务设置 model: sonnet",
            "Set model: sonnet in the definitions of subagents that do simple subtasks",
        ),
        "expensive_subagent" => lang.pick(
            "简单子任务换用更便宜的模型",
            "Use a cheaper model for simple subtasks",
        ),
        "heavy_reasoning" | "high_effort_light_task" => lang.pick(
            "简单任务调低推理强度",
            "Lower the reasoning effort for simple tasks",
        ),
        "fast_mode" => lang.pick(
            "不赶时间时关闭快速模式",
            "Turn off fast mode when speed is not essential",
        ),
        "frequent_compaction" => lang.pick(
            "一个会话只做一件事，阶段完成后开启新会话",
            "Keep one task per session and start a new session after each stage",
        ),
        "web_search" => lang.pick(
            "已知网址直接提供，文档类问题先查本地文件",
            "Provide known URLs directly and check local files first for documentation questions",
        ),
        "quota_rebalance" => lang.pick(
            "可检查哪些任务适合交给另一工具",
            "Check which tasks could move to the other tool",
        ),
        "plan_fit" => lang.pick(
            "可在账号的订阅设置里核对套餐和价格",
            "Check your plan and price in your account's subscription settings",
        ),
        _ => return None,
    })
}

pub(super) fn next_step(
    tool: Tool,
    id: &str,
    lang: Lang,
    config: &AdviceConfig,
) -> Option<NextStep> {
    text(tool, id, lang).map(|text| NextStep {
        text: text.to_owned(),
        command: command(tool, id, config).map(str::to_owned),
    })
}
