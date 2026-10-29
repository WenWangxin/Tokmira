import type { Overview, SessionOverview, TokenCounts, Tool } from '../api'

// M7B_DEV_PREVIEW_ONLY: handwritten, synthetic data for the browser preview.
const now = new Date()
const at = (minutes: number) => new Date(now.getTime() + minutes * 60_000).toISOString()
const day = (offset: number) => {
  const date = new Date(now)
  date.setDate(date.getDate() + offset)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}
const tokens = (input: number, cache_read: number, output: number): TokenCounts => ({
  input, output, reasoning: 0, cache_read, cache_write_5m: 0,
  cache_write_1h: 0, cache_write_unspecified: 0,
})
const session = (tool: Tool, id: string, title: string, source: SessionOverview['source'], model: string,
  amount: TokenCounts, hit: number, weekly: number): SessionOverview => ({
  tool, session_id: id, title, source, project: '/sample/workspace', model, requests: 24,
  tokens: amount, cache_hit_rate: hit, five_hour_delta: weekly / 2, weekly_delta: weekly,
  total_tokens: amount.input + amount.cache_read + amount.output,
  first_at: at(-260), last_at: at(-12),
})
const series = (values: number[], start: number, step: number) => values.map((percent, index) => ({
  at: at(start + index * step), percent,
}))

export function mockOverview(variant: 'default' | 'empty' | 'nodata' | 'hit' | 'hit-estimated'): Overview {
  const noQuota = variant === 'nodata'
  const empty = variant === 'empty'
  const sessions = [
    session('Codex', 'sample-codex-01', '迁移数据库脚本', 'CodexDesktop', 'gpt-6-sol', tokens(370000, 1120000, 140000), .64, 11.2),
    session('Claude', 'sample-claude-02', '重构登录模块', 'ClaudeCli', 'claude-opus-5-5', tokens(170000, 930000, 140000), .71, 9.6),
    session('Codex', 'sample-codex-03', '写 README', 'CodexCli', 'gpt-6-sol', tokens(90000, 710000, 130000), .82, 6.4),
    session('Claude', 'sample-claude-04', '写对账单元测试', 'ClaudeDesktop', 'claude-opus-5-5', tokens(180000, 350000, 230000), .48, 5.8),
    session('Codex', 'sample-codex-05', '检查命令行参数', 'CodexCli', 'gpt-6-sol', tokens(43000, 121000, 42000), .58, 2.1),
  ]
  const quota = (used: number, reset: number, forecast: Overview['tools'][number]['five_hour']['forecast'], origin: Overview['tools'][number]['five_hour']['origin'], estimated: number | null = null) => ({
    used_percent: noQuota || empty ? null : used,
    resets_at: noQuota || empty ? null : at(reset),
    origin: noQuota || empty ? null : origin,
    forecast: noQuota || empty ? 'Unknown' as const : forecast,
    window_passed: false,
    as_of: noQuota || empty ? null : at(-3),
    estimated_percent: noQuota || empty ? null : estimated,
  })
  const claudePoints = series([0, 4, 6, 9, 11, 13, 17, 21, 24, 28], -300, 30)
  const codexPoints = series([0, 2, 4, 8, 11, 16, 21], -300, 45)
  const marker = (minutes: number, delta: number, title: string) => ({ at: at(minutes), delta, session_id: 'sample-claude-02', title })
  return {
    generated_at: now.toISOString(),
    tools: [
      { tool: 'Claude', plan: 'pro', subscription_usd: 20,
        // `hit`: a 97% official reading pins the card; `hit-estimated`: a 90% reading plus a 6% estimate does not.
        five_hour: variant === 'hit' ? quota(97, 16, { WillHit: { at: at(8) } }, 'Official')
          : variant === 'hit-estimated' ? quota(96, 16, { WillHit: { at: at(8) } }, 'Official', 6)
          : quota(28, 16, 'Ample', 'Official', 2.4), weekly: quota(6, 6 * 24 * 60, 'Unknown', 'Official', 0.3) },
      { tool: 'Codex', plan: 'plus', subscription_usd: 20,
        five_hour: quota(21, 52, { WillHit: { at: at(100) } }, 'Log'), weekly: quota(3, 7 * 24 * 60, 'Unknown', 'Log') },
    ],
    today_tokens: empty ? tokens(0, 0, 0) : tokens(640000, 1940000, 150000),
    today_token_total: empty ? 0 : 2730000,
    today_cache_read_share: empty ? null : .68,
    peak_hour: empty ? null : 14,
    hourly_tokens: empty ? Array(24).fill(0) : [0, 0, 0, 0, 0, 0, 20000, 16000, 35000, 40000, 65000, 90000, 120000, 180000, 260000, 230000, 200000, 260000, 220000, 200000, 170000, 130000, 70000, 30000],
    today_cache_hit_rate: empty ? null : .68,
    daily_cache_hit_rate: Array.from({ length: 14 }, (_, index) => ({ date: day(index - 13), rate: empty ? null : [.43, .47, .45, .53, .49, .57, .55, .61, .57, .65, .63, .67, .64, .68][index] ?? null })),
    cache_hit_change_from_yesterday: empty ? null : .06,
    month_estimated_cost_usd: empty ? 0 : 187.4,
    month_cost_by_tool: empty ? {} : { Claude: 100, Codex: 87.4 },
    subscription_total_usd: 40,
    subscription_value_multiple: empty ? 0 : 4.685,
    daily_cost: Array.from({ length: 14 }, (_, index) => ({ date: day(index - 13), estimated_cost_usd: empty ? 0 : [4, 6, 5, 8, 7, 11, 9, 13, 12, 15, 13, 18, 16, 20][index] ?? 0 })),
    recorder: { statusline_installed: true, statusline_modified: false, hooks_installed: true, hooks_modified: false,
      statusline_records: 19, hook_events: 38, last_statusline_record: at(-8), last_hook_event: at(-14) },
    quota_sources: { claude_official: noQuota || empty ? null : at(-3), codex_official: null, codex_log: noQuota || empty ? null : at(-11) },
    trends: [
      { tool: 'Claude', five_hour_started_at: at(-300), five_hour: empty || noQuota ? [] : claudePoints,
        high_prompts: empty || noQuota ? [] : [marker(-270, 4, '初始化模块'), marker(-240, 3.6, '重构登录模块'), marker(-120, 3.9, '检查缓存'), marker(-90, 4.2, '写对账测试')],
        today: empty || noQuota ? [] : claudePoints, today_high_prompts: empty || noQuota ? [] : [marker(-120, 3.9, '检查缓存')],
        week_started_at: at(-6 * 24 * 60), week: empty || noQuota ? [] : series([0, 1, 2, 3, 4, 5, 6], -6 * 24 * 60, 24 * 60),
        week_high_prompts: empty || noQuota ? [] : [marker(-120, 3.9, '检查缓存')] },
      { tool: 'Codex', five_hour_started_at: at(-300), five_hour: empty || noQuota ? [] : codexPoints,
        high_prompts: empty || noQuota ? [] : [marker(-180, 2.1, '迁移数据库脚本')],
        today: empty || noQuota ? [] : codexPoints, today_high_prompts: [],
        week_started_at: at(-6 * 24 * 60), week: empty || noQuota ? [] : series([0, .4, 1, 1.8, 2, 2.5, 3], -6 * 24 * 60, 24 * 60), week_high_prompts: [] },
    ],
    recent_sessions: empty ? [] : sessions,
    recent_high_prompts: empty ? [] : sessions.slice(0, 5).map((item, index) => ({
      at: at(-30 - index * 60), tool: item.tool, source: item.source, session_id: item.session_id,
      title: item.title, project: item.project, model: item.model, tokens: item.tokens,
      total_tokens: item.total_tokens,
      cache_hit_rate: item.cache_hit_rate, five_hour_delta: item.five_hour_delta,
      weekly_delta: item.weekly_delta,
    })),
    summary: { sessions: empty ? 0 : 7, requests: empty ? 0 : 1284, high_prompts: empty ? 0 : 5 },
    quota_hit_threshold_pct: 95,
  }
}
