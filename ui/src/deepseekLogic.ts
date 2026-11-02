import type { DeepSeekOverview, DeepSeekUsage } from './api'
import { m } from './i18n/index.ts'

/** Yuan with two decimals; a non-zero amount below a fen still shows as spent. */
export function yuan(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—'
  if (value > 0 && value < 0.005) return '<¥0.01'
  return `${value < 0 ? '-' : ''}¥${Math.abs(value).toFixed(2)}`
}

/** "约 27 天" for the balance card, or a dash when there is no pace or balance. */
export function daysLeftText(overview: DeepSeekOverview): string {
  if (!overview.balance) return '—'
  if (overview.days_left === null) return m().deepseek.noRecentSpend
  if (overview.days_left < 1) return m().deepseek.underDay
  return m().deepseek.daysLeft(Math.round(overview.days_left))
}

/** Green while comfortable, amber when low, red when spent. */
export function daysTone(overview: DeepSeekOverview): '' | 'success-text' | 'warning-text' | 'error-text' {
  if (!overview.balance || overview.days_left === null) return ''
  if (overview.balance.total <= 0) return 'error-text'
  return overview.low_balance ? 'warning-text' : 'success-text'
}

export interface CostSegment {
  key: string
  label: string
  cost: number
  kind: 'session' | 'other' | 'title'
}

/** The stacked bar: the costliest sessions, the rest together, then estimated title requests. */
export function costSegments(usage: DeepSeekUsage, shown = 3): CostSegment[] {
  const sessions = usage.sessions.filter(item => item.cost > 0)
  const segments: CostSegment[] = sessions.slice(0, shown)
    .map(item => ({ key: item.key, label: item.name, cost: item.cost, kind: 'session' }))
  const rest = sessions.slice(shown).reduce((sum, item) => sum + item.cost, 0)
  if (rest > 0) segments.push({ key: 'other', label: m().deepseek.otherSessions, cost: rest, kind: 'other' })
  if (usage.title_cost > 0) segments.push({ key: 'title', label: m().deepseek.titleGeneration, cost: usage.title_cost, kind: 'title' })
  return segments
}

/** Bar widths as shares of the period's spend: where the money went, not how much is left. */
export function segmentWidths(segments: CostSegment[]): number[] {
  const spent = segments.reduce((sum, item) => sum + item.cost, 0)
  return segments.map(item => spent > 0 ? item.cost / spent * 100 : 0)
}
