import { quotaMessageTone } from './settingsLogic.ts'
import type { QuotaOutcome, RefreshResult } from './api'
import { m, monthDay } from './i18n/index.ts'

export type SourceTone = 'ok' | 'neutral' | 'warning'
export type SourceAction = 'authorize' | 'settings' | null

export interface SourceRow {
  key: string
  tool: 'Claude' | 'Codex'
  title: string
  status: string
  tone: SourceTone
  action: SourceAction
}

export interface SourceStatusInput {
  claudeOfficialEnabled: boolean
  codexOfficialEnabled: boolean
  recorder: { statusline_installed: boolean; statusline_modified: boolean; last_statusline_record: string | null }
  times: { claude_official: string | null; codex_official: string | null; codex_log: string | null } | null
  claudeNeedsAuthorization: boolean
  lastResult: RefreshResult | null
  now: number
}

/** “刚刚”, “5 分钟前”, “3 小时前”, or a date for anything older than a day. */
export function timeAgo(value: string, now: number): string {
  const minutes = Math.max(0, Math.floor((now - Date.parse(value)) / 60_000))
  if (minutes < 1) return m().time.justNow
  if (minutes < 60) return m().time.minutesAgo(minutes)
  if (minutes < 24 * 60) return m().time.hoursAgo(Math.floor(minutes / 60))
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${monthDay(date)} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** A query error worth showing; quiet outcomes such as “60 秒内已查询” are not. */
function failure(outcome: QuotaOutcome | undefined): string | null {
  return outcome && quotaMessageTone(outcome) === 'bad' ? outcome.message : null
}

/** One row per quota source: whether it is on, when it last delivered, and what to do next. */
export function sourceRows(input: SourceStatusInput): SourceRow[] {
  const { times, now } = input
  const t = m().sourceStatus
  const updated = (at: string | null | undefined, verb: (ago: string) => string = m().time.updated) => (at ? verb(timeAgo(at, now)) : null)
  const rows: SourceRow[] = []

  const claudeError = failure(input.lastResult?.claude)
  rows.push({
    key: 'claudeOfficial', tool: 'Claude', title: t.claudeOfficial,
    ...(!input.claudeOfficialEnabled
      ? { status: t.off, tone: 'neutral', action: 'settings' }
      : input.claudeNeedsAuthorization
        ? { status: t.needsAuthorization, tone: 'warning', action: 'authorize' }
        : claudeError
          ? { status: claudeError, tone: 'warning', action: null }
          : updated(times?.claude_official)
            ? { status: updated(times?.claude_official)!, tone: 'ok', action: null }
            : { status: t.noData, tone: 'neutral', action: null }),
  } as SourceRow)

  const recorder = input.recorder
  rows.push({
    key: 'statusline', tool: 'Claude', title: t.statusline,
    ...(!recorder.statusline_installed
      ? { status: t.notInstalled, tone: 'neutral', action: 'settings' }
      : recorder.statusline_modified
        ? { status: t.statuslineChanged, tone: 'warning', action: 'settings' }
        : recorder.last_statusline_record
          ? { status: updated(recorder.last_statusline_record, m().time.received)!, tone: 'ok', action: null }
          : { status: t.statuslineWaiting, tone: 'neutral', action: null }),
  } as SourceRow)

  const codexError = failure(input.lastResult?.codex)
  rows.push({
    key: 'codexOfficial', tool: 'Codex', title: t.codexOfficial,
    ...(!input.codexOfficialEnabled
      ? { status: t.off, tone: 'neutral', action: 'settings' }
      : codexError
        ? { status: codexError, tone: 'warning', action: null }
        : updated(times?.codex_official)
          ? { status: updated(times?.codex_official)!, tone: 'ok', action: null }
          : { status: t.noData, tone: 'neutral', action: null }),
  } as SourceRow)

  rows.push({
    key: 'codexLog', tool: 'Codex', title: t.codexLog,
    ...(updated(times?.codex_log, m().time.recorded)
      ? { status: updated(times?.codex_log, m().time.recorded)!, tone: 'ok', action: null }
      : { status: t.noQuotaRecords, tone: 'neutral', action: null }),
  } as SourceRow)
  return rows
}

/** The status button turns amber when any source needs attention or loading failed. */
export function overallTone(rows: SourceRow[], loadError: string): 'success' | 'warning' {
  return loadError || rows.some(row => row.tone === 'warning') ? 'warning' : 'success'
}
