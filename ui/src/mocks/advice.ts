import type { AdviceItem, AdviceReport, Tool } from '../api'
import { locale } from '../i18n/index.ts'

// Handwritten translations of the demo cases, not a second diagnosis engine.
const english: Record<string, Pick<AdviceItem, 'title' | 'facts' | 'description'>> = {
  cache_rebuild: {
    title: 'Full cache rebuild after expiry',
    facts: ['9 occurrences', '389K tokens rebuilt in total', 'Gaps mostly 62–95 minutes'],
    description: 'After a gap longer than the recorded cache lifetime, the context needs to be written again. Consider continuing related work before expiry, or start a new session with only the context you need.',
  },
  expensive_light_task: {
    title: 'Light tasks used Opus', facts: ['48 prompts', 'Average output: 310 tokens', 'Estimated cost reduction with Sonnet: 62%'],
    description: 'For short tasks such as renaming, adding comments or running tests, consider Sonnet and check the result. In Claude Code, use /model sonnet.',
  },
  session_startup: {
    title: 'New-session startup overhead', facts: ['21 new sessions', 'About 45K tokens per session'],
    description: 'New sessions write system instructions and tool definitions into the cache. Related small tasks can share a session to avoid repeating that overhead.',
  },
  model_switch: { title: 'Model switches rebuilt the cache', facts: ['Synthetic preview only'], description: 'Switch between task phases, or start a new session.' },
  context_spike: { title: 'A large amount of context at once', facts: ['Synthetic preview only'], description: 'Read only the relevant parts of large files and split large diffs.' },
  runaway_prompt: { title: 'Too many turns from one prompt', facts: ['Synthetic preview only'], description: 'Define completion criteria before running the task.' },
  expensive_subagent: { title: 'Subagents used an expensive model', facts: ['Synthetic preview only'], description: 'Consider Sonnet for simple subtasks.' },
  fast_mode: { title: 'Fast mode was used', facts: ['Synthetic preview only'], description: 'Turn off /fast when the task is not urgent.' },
  high_effort_light_task: { title: 'Light tasks used high reasoning effort', facts: ['Synthetic preview only'], description: 'Consider lower reasoning effort for light tasks.' },
  frequent_compaction: { title: 'Frequent context compaction', facts: ['Synthetic preview only'], description: 'Consider a fresh session when a task phase is complete.' },
  web_search: { title: 'Frequent web searches', facts: ['Synthetic preview only'], description: 'Check local documentation first when it can answer the question.' },
  quota_rebalance: { title: "Use the other tool's available quota", facts: ['Synthetic preview only'], description: 'Move suitable tasks to the tool with more quota available.' },
  plan_fit: { title: 'Check your plan fit', facts: ['Last 4 complete weeks: 35%, 42%, 39%, 31%', 'Estimated monthly fee reduction: $80'], description: 'Evaluate the last four complete weeks.' },
  long_context: { title: 'Long context', facts: ['80 requests', 'Average context: 331K', 'Largest context: 480K'], description: 'Longer context means reading more cached tokens on each request. Consider /compact or a fresh session after completing a task phase.' },
  heavy_reasoning: { title: 'High reasoning-token share', facts: ['3 matching prompts', '12K reasoning tokens'], description: 'Consider lower reasoning effort via /model for simpler tasks.' },
}

// Next steps as core sends them for the preview cases: [zh, en, command]. Commands only
// where core has a verified one (Claude); Codex stays text-only.
const steps: Record<string, [string, string, string | null]> = {
  'claude:cache_rebuild': ['离开超过缓存有效期后，开启新会话并只带必要上下文', 'After a break longer than the cache lifetime, start a new session with only the context you need', null],
  'claude:expensive_light_task': ['做短小任务前先切换到 Sonnet', 'Switch to Sonnet before short tasks', '/model sonnet'],
  'claude:session_startup': ['可检查不常用的 MCP 服务器和插件，连续的小任务放在同一会话', 'Review MCP servers and plugins you rarely use, and keep related small tasks in one session', null],
  'claude:model_switch': ['换模型放在任务阶段之间，或换模型后开启新会话', 'Switch models between task stages, or start a new session after switching', null],
  'claude:context_spike': ['大文件和日志只读取相关部分，大 diff 分批查看', 'Read only the relevant parts of large files and logs, and review large diffs in batches', null],
  'claude:runaway_prompt': ['先写清完成条件；反复修同一个错误时打断并缩小范围', 'State completion criteria first; interrupt and narrow the scope if the same error keeps coming back', null],
  'claude:expensive_subagent': ['在子代理定义里给简单子任务设置 model: sonnet', 'Set model: sonnet in the definitions of subagents that do simple subtasks', null],
  'claude:fast_mode': ['不赶时间时关闭快速模式', 'Turn off fast mode when speed is not essential', null],
  'claude:high_effort_light_task': ['简单任务调低推理强度', 'Lower the reasoning effort for simple tasks', '/effort high'],
  'claude:frequent_compaction': ['一个会话只做一件事，阶段完成后开启新会话', 'Keep one task per session and start a new session after each stage', null],
  'claude:web_search': ['已知网址直接提供，文档类问题先查本地文件', 'Provide known URLs directly and check local files first for documentation questions', null],
  'claude:quota_rebalance': ['可检查哪些任务适合交给另一工具', 'Check which tasks could move to the other tool', null],
  'claude:plan_fit': ['可在账号的订阅设置里核对套餐和价格', "Check your plan and price in your account's subscription settings", null],
  'codex:long_context': ['任务阶段完成后压缩上下文，或开启新会话', 'After a task stage, compact the context or start a new session', null],
  'codex:heavy_reasoning': ['简单任务调低推理强度', 'Lower the reasoning effort for simple tasks', null],
}

// Synthetic browser preview only; no user logs or credentials.
const item = (tool: Tool, id: string, partial: Partial<AdviceItem>): AdviceItem => ({
  id,
  key: `${tool.toLowerCase()}:${id}`,
  tool,
  title: '',
  severity: 'Warning',
  facts: [],
  description: '',
  occurrences: 1,
  sessions: 1,
  overhead_tokens: 0,
  overhead_cost_usd: 0,
  consumed_weekly_pct: null,
  avoidable: true,
  avoidable_cost_usd: 0,
  avoidable_weekly_pct: null,
  link: null,
  next_step: null,
  latest_session: { tool, source: tool === 'Claude' ? 'ClaudeCli' : 'CodexCli', session_id: `demo-${id}`, project: '/Users/demo/token-audit' },
  ...partial,
})

export function mockAdvice(periodDays: number, variant: string | null): AdviceReport {
  const empty = variant === 'empty'
  const noQuota = variant === 'nodata'
  const pct = (value: number) => (noQuota ? null : value)
  const claude = empty ? [] : [
    item('Claude', 'cache_rebuild', {
      title: '缓存过期后的整段重建', severity: 'Error', occurrences: 9, sessions: 4,
      facts: ['发生 9 次', '累计重建 38.9 万 token', '间隔多为 62–95 分钟'],
      description: '订阅版缓存的有效期为 1 小时。间隔超过 1 小时再继续对话时，之前的上下文需要整段重新写入缓存。建议在过期前继续，或开启新会话并只保留必要上下文。',
      overhead_tokens: 389000, overhead_cost_usd: 3.03, consumed_weekly_pct: pct(4.1), avoidable_cost_usd: 3.03, avoidable_weekly_pct: pct(4.1),
      link: { label: '如何将缓存命中率从 52% 提升至 80%', url: 'https://example.invalid/cache-guide' },
    }),
    item('Claude', 'expensive_light_task', {
      title: '轻量任务使用了 Opus', occurrences: 48, sessions: 6,
      facts: ['48 次提示', '平均输出 310 token', '改用 Sonnet 约省 62%'],
      description: '重命名、补注释、运行测试等短小任务，换用 Sonnet 效果相近、额度消耗更低。Claude Code 中可执行 /model sonnet。',
      overhead_tokens: 820000, overhead_cost_usd: 1.9, consumed_weekly_pct: pct(2.6), avoidable_cost_usd: 1.9, avoidable_weekly_pct: pct(2.6),
    }),
    item('Claude', 'session_startup', {
      title: '新会话的启动开销', occurrences: 21, sessions: 21, avoidable: false,
      facts: ['21 个新会话', '单次约 4.5 万 token'],
      description: '每个新会话都需要先把系统提示和工具定义写入缓存。频繁新开会话会反复支付这部分开销；连续的小任务可以在同一会话里完成。',
      overhead_tokens: 945000, overhead_cost_usd: 4.72, consumed_weekly_pct: pct(3.4), avoidable_cost_usd: 0, avoidable_weekly_pct: pct(0),
    }),
    ...[
      ['model_switch', '中途换模型导致缓存重建', '换模型放在任务阶段之间，或开启新会话。'],
      ['context_spike', '一次塞入大量内容', '大文件只读相关部分，大 diff 分批看。'],
      ['runaway_prompt', '一次提示跑了太多轮', '先明确完成条件，再执行。'],
      ['expensive_subagent', '子代理用了高价模型', '简单子任务可选用 Sonnet。'],
      ['fast_mode', '使用了快速模式', '不赶时间时用 /fast 关闭。'],
      ['high_effort_light_task', '轻量任务用了高推理强度', '轻量任务可调低推理强度。'],
      ['frequent_compaction', '频繁压缩上下文', '阶段完成后可以开启新会话。'],
      ['web_search', '联网搜索次数较多', '文档问题优先查本地。'],
      ['quota_rebalance', '两个额度池可以分流', '适合的任务可以分流到另一工具。'],
      ['plan_fit', '套餐是否合适', '根据最近四个完整周评估。'],
    ].map(([id, title, description]) => item('Claude', id, {
      title, description, avoidable: false, sessions: id === 'plan_fit' || id === 'quota_rebalance' ? 0 : 2,
      facts: id === 'plan_fit' ? ['最近 4 个完整周：35%、42%、39%、31%', '降档后月费减少约 $80'] : ['仅供开发预览'],
    })),
  ]
  const codex = empty ? [] : [
    item('Codex', 'long_context', {
      title: '上下文过长', occurrences: 80, sessions: 3,
      facts: ['80 次请求', '平均上下文 33.1 万', '最长 48.0 万'],
      description: '上下文越长，每次请求都要重新读取越多缓存。任务阶段完成后可执行 /compact，或开启新会话。',
      overhead_tokens: 1400000, overhead_cost_usd: 0.28, consumed_weekly_pct: pct(2.4), avoidable_cost_usd: 0.28, avoidable_weekly_pct: pct(2.4),
    }),
    item('Codex', 'heavy_reasoning', {
      title: '推理 token 占比过高', facts: ['命中 3 次提示', '推理 1.2 万 token'],
      description: '简单任务可通过 /model 调低推理强度。', overhead_tokens: 12000,
      overhead_cost_usd: 0.09, avoidable_cost_usd: 0.09,
    }),
  ]
  for (const advice of [...claude, ...codex]) {
    const step = steps[advice.key]
    if (step) advice.next_step = { text: locale.value === 'en' ? step[1] : step[0], command: step[2] }
  }
  if (locale.value === 'en') {
    for (const advice of [...claude, ...codex]) {
      Object.assign(advice, english[advice.id])
      if (advice.link) advice.link.label = 'Understanding cache hits — demo link'
    }
  }
  const summary = (items: AdviceItem[]) => ({
    avoidable_weekly_pct: noQuota ? null : items.reduce((sum, advice) => sum + (advice.avoidable_weekly_pct ?? 0), 0),
    avoidable_cost_usd: items.reduce((sum, advice) => sum + advice.avoidable_cost_usd, 0),
    count: items.length,
    sessions: Math.min(18, items.reduce((sum, advice) => sum + advice.sessions, 0)),
  })
  return {
    generated_at: new Date().toISOString(),
    period_days: periodDays,
    config_version: 'preview',
    tools: [
      { tool: 'Claude', has_quota_data: !noQuota, summary: summary(claude), items: claude, hidden: 0 },
      { tool: 'Codex', has_quota_data: !noQuota, summary: summary(codex), items: codex, hidden: 0 },
    ],
    prompt_tags: [],
    event_tags: [],
  }
}
