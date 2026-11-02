import type { CostRank, DeepSeekAdvice, DeepSeekPeriod, DeepSeekUsage, TokenCounts } from '../api'
import { mockDeepSeekOverview } from './overview'

// Synthetic browser preview only; no user logs or credentials.
const now = new Date()
const at = (minutes: number) => new Date(now.getTime() + minutes * 60_000).toISOString()
const startOf = (period: DeepSeekPeriod) => {
  const date = new Date(now)
  date.setHours(0, 0, 0, 0)
  if (period === 'week') date.setDate(date.getDate() - 6)
  if (period === 'month') date.setDate(1)
  return date.toISOString()
}
const tokens = (input: number, cache_read: number, output: number): TokenCounts => ({
  input, output, reasoning: 0, cache_read, cache_write_5m: 0, cache_write_1h: 0, cache_write_unspecified: 0,
})
const rank = (key: string, name: string, project_name: string, tokens: number, cost: number, requests = 6): CostRank =>
  ({ key, name, project_name, tokens, requests, cost })

export function mockDeepSeekUsage(period: DeepSeekPeriod, variant: string | null): DeepSeekUsage {
  const empty = variant === 'empty' || variant === 'nodata'
  const scale = period === 'today' ? 1 : period === 'week' ? 5.4 : 11
  const sessions = empty ? [] : [
    rank('session-a', '重构支付模块', 'sample-shop', 410_000 * scale, 1.42 * scale, 14),
    rank('session-b', '生成周报', 'sample-shop', 260_000 * scale, 0.86 * scale, 9),
    rank('session-c', '排查构建失败', 'token-audit', 150_000 * scale, 0.51 * scale, 7),
    rank('session-d', '补充单元测试', 'token-audit', 80_000 * scale, 0.29 * scale, 4),
  ]
  const requestCost = sessions.reduce((sum, item) => sum + item.cost, 0)
  const title = empty ? 0 : 0.12 * scale
  return {
    period,
    start: startOf(period),
    end: now.toISOString(),
    cost: requestCost + title,
    requests: sessions.reduce((sum, item) => sum + item.requests, 0),
    tokens: empty ? tokens(0, 0, 0) : tokens(310_000 * scale, 560_000 * scale, 30_000 * scale),
    cache_hit_rate: empty ? null : 0.64,
    title_cost: title,
    title_requests: empty ? 0 : Math.round(6 * scale),
    unpriced_requests: 0,
    sessions,
    projects: empty ? [] : [
      rank('/sample/sample-shop', 'sample-shop', '', 670_000 * scale, 2.28 * scale, 23),
      rank('/sample/token-audit', 'token-audit', '', 230_000 * scale, 0.8 * scale, 11),
    ],
    models: empty ? [] : [
      rank('deepseek-v4-pro', 'deepseek-v4-pro', '', 520_000 * scale, 2.4 * scale, 12),
      rank('deepseek-flash', 'deepseek-flash', '', 380_000 * scale, 0.68 * scale, 22),
    ],
    subagents: empty ? [] : [
      rank('main', '主对话', '', 760_000 * scale, 2.71 * scale, 28),
      rank('subagent', '子代理', '', 140_000 * scale, 0.37 * scale, 6),
    ],
    high_requests: empty ? [] : [
      { at: at(-50), session_id: 'session-a', title: '重构支付模块', model: 'deepseek-v4-pro', tokens: 61_000, cost: 0.21 },
      { at: at(-78), session_id: 'session-a', title: '重构支付模块', model: 'deepseek-v4-pro', tokens: 52_000, cost: 0.17 },
      { at: at(-145), session_id: 'session-b', title: '生成周报', model: 'deepseek-v4-pro', tokens: 44_000, cost: 0.14 },
      { at: at(-162), session_id: 'session-c', title: '排查构建失败', model: 'deepseek-flash', tokens: 98_000, cost: 0.11 },
    ],
    overview: mockDeepSeekOverview(),
  }
}

export function mockDeepSeekAdvice(periodDays: number, variant: string | null): DeepSeekAdvice {
  if (variant === 'empty' || variant === 'nodata') return { period_days: periodDays, saving_per_day: 0, sessions: 0, tips: [] }
  const tips: DeepSeekAdvice['tips'] = [
    { key: 'deepseek:log_upload', kind: 'log_upload', sessions: 1, requests: 0, tokens: 0, cost_per_day: 0, saving_per_day: 0, model: null },
    { key: 'deepseek:title_requests', kind: 'title_requests', sessions: 5, requests: 5, tokens: 1_400, cost_per_day: 0.12, saving_per_day: 0.12, model: 'deepseek-flash' },
    { key: 'deepseek:cold_start', kind: 'cold_start', sessions: 1, requests: 1, tokens: 9_007, cost_per_day: 0, saving_per_day: 0.3, model: null },
  ]
  return { period_days: periodDays, saving_per_day: tips.reduce((sum, tip) => sum + tip.saving_per_day, 0), sessions: 5, tips }
}
