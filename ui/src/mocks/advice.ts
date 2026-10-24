import type { AdviceItem, AdviceReport, Tool } from '../api'

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
  ]
  const codex = empty ? [] : [
    item('Codex', 'long_context', {
      title: '上下文过长', occurrences: 80, sessions: 3,
      facts: ['80 次请求', '平均上下文 33.1 万', '最长 48.0 万'],
      description: '上下文越长，每次请求都要重新读取越多缓存。任务阶段完成后可执行 /compact，或开启新会话。',
      overhead_tokens: 1400000, overhead_cost_usd: 0.28, consumed_weekly_pct: pct(2.4), avoidable_cost_usd: 0.28, avoidable_weekly_pct: pct(2.4),
    }),
  ]
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
  }
}
