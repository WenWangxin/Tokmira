import type { AdviceItem, AdviceReport, Origin, QuotaDisplay, Tool, ToolOverview, UsageDetail, UsageRank, WindowKind } from './api'

/** Mirrors `[quota_hit] threshold_pct` in `config/advice.toml`; used only when an old overview lacks it. */
export const DEFAULT_HIT_THRESHOLD = 95

export interface QuotaHit {
  tool: Tool
  kind: WindowKind
  /** The latest real reading, without the estimate added after it. */
  reading: number
  origin: Origin
  as_of: string
  resets_at: string | null
}

const READING_ORIGINS: readonly Origin[] = ['Official', 'Statusline', 'Log', 'Manual']

/** The latest real reading; `null` when there is none or the window has already reset. */
export function realReading(quota: QuotaDisplay, now = Date.now()): number | null {
  if (quota.used_percent === null || quota.window_passed || quota.as_of === null) return null
  if (quota.origin === null || !READING_ORIGINS.includes(quota.origin)) return null
  if (quota.resets_at && Date.parse(quota.resets_at) <= now) return null
  return quota.used_percent - (quota.estimated_percent ?? 0)
}

/** Each tool and each window on its own; an estimate reaching the threshold never counts. */
export function quotaHits(tools: ToolOverview[], threshold: number, now = Date.now()): QuotaHit[] {
  return tools.flatMap(item => ([['FiveHour', item.five_hour], ['Weekly', item.weekly]] as const).flatMap(([kind, quota]) => {
    const reading = realReading(quota, now)
    if (reading === null || reading < threshold) return []
    return [{ tool: item.tool, kind, reading, origin: quota.origin as Origin, as_of: quota.as_of as string, resets_at: quota.resets_at }]
  }))
}

export type HitRow =
  | { kind: 'session'; key: string; name: string; delta: number }
  | { kind: 'subagent'; delta: number }
  | { kind: 'unattributed'; delta: number }

/**
 * Top three sessions, the subagent total, and “Unattributed” when it outweighs the third
 * session. The subagent ranking only has `main` and `subagent`, so main is skipped.
 */
export function hitRows(detail: UsageDetail, limit = 3): HitRow[] {
  const sessions: UsageRank[] = detail.sessions.filter(row => row.delta > 0).slice(0, limit)
  const rows: HitRow[] = sessions.map(row => ({ kind: 'session', key: row.key, name: row.name, delta: row.delta }))
  const subagent = detail.subagents.find(row => row.key === 'subagent')
  if (subagent && subagent.delta > 0) rows.push({ kind: 'subagent', delta: subagent.delta })
  const third = sessions.length < limit ? 0 : sessions[limit - 1]!.delta
  if (detail.unattributed_total > 0 && detail.unattributed_total > third) rows.push({ kind: 'unattributed', delta: detail.unattributed_total })
  return rows
}

/** The same tool's avoidable advice with the largest weekly share. */
export function hitAdvice(report: AdviceReport | null, tool: Tool): AdviceItem | null {
  const items = report?.tools.find(item => item.tool === tool)?.items
    .filter(item => item.avoidable && (item.avoidable_weekly_pct ?? 0) > 0) ?? []
  return items.slice().sort((a, b) => (b.avoidable_weekly_pct ?? 0) - (a.avoidable_weekly_pct ?? 0)
    || b.avoidable_cost_usd - a.avoidable_cost_usd)[0] ?? null
}
