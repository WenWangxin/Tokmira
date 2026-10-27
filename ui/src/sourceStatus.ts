import { quotaMessageTone } from './settingsLogic.ts'

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
  lastResult: { claude: string; codex: string } | null
  now: number
}

/** “刚刚”, “5 分钟前”, “3 小时前”, or a date for anything older than a day. */
export function timeAgo(value: string, now: number): string {
  const minutes = Math.max(0, Math.floor((now - Date.parse(value)) / 60_000))
  if (minutes < 1) return '刚刚'
  if (minutes < 60) return `${minutes} 分钟前`
  if (minutes < 24 * 60) return `${Math.floor(minutes / 60)} 小时前`
  const date = new Date(value)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getMonth() + 1}/${date.getDate()} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** A query error worth showing; quiet outcomes such as “60 秒内已查询” are not. */
function failure(message: string | undefined): string | null {
  return message && quotaMessageTone(message) === 'bad' ? message : null
}

/** One row per quota source: whether it is on, when it last delivered, and what to do next. */
export function sourceRows(input: SourceStatusInput): SourceRow[] {
  const { times, now } = input
  const updated = (at: string | null | undefined, verb = '更新') => (at ? `${timeAgo(at, now)}${verb}` : null)
  const rows: SourceRow[] = []

  const claudeError = failure(input.lastResult?.claude)
  rows.push({
    key: 'claudeOfficial', tool: 'Claude', title: 'Claude 官方查询',
    ...(!input.claudeOfficialEnabled
      ? { status: '未开启', tone: 'neutral', action: 'settings' }
      : input.claudeNeedsAuthorization
        ? { status: '需要授权才能自动获取', tone: 'warning', action: 'authorize' }
        : claudeError
          ? { status: claudeError, tone: 'warning', action: null }
          : updated(times?.claude_official)
            ? { status: updated(times?.claude_official)!, tone: 'ok', action: null }
            : { status: '暂无数据', tone: 'neutral', action: null }),
  } as SourceRow)

  const recorder = input.recorder
  rows.push({
    key: 'statusline', tool: 'Claude', title: 'Claude Code 状态栏记录',
    ...(!recorder.statusline_installed
      ? { status: '未安装', tone: 'neutral', action: 'settings' }
      : recorder.statusline_modified
        ? { status: '状态栏配置已被改动', tone: 'warning', action: 'settings' }
        : recorder.last_statusline_record
          ? { status: updated(recorder.last_statusline_record, '收到')!, tone: 'ok', action: null }
          : { status: '尚未收到（仅命令行版有效）', tone: 'neutral', action: null }),
  } as SourceRow)

  const codexError = failure(input.lastResult?.codex)
  rows.push({
    key: 'codexOfficial', tool: 'Codex', title: 'Codex 官方额度',
    ...(!input.codexOfficialEnabled
      ? { status: '未开启', tone: 'neutral', action: 'settings' }
      : codexError
        ? { status: codexError, tone: 'warning', action: null }
        : updated(times?.codex_official)
          ? { status: updated(times?.codex_official)!, tone: 'ok', action: null }
          : { status: '暂无数据', tone: 'neutral', action: null }),
  } as SourceRow)

  rows.push({
    key: 'codexLog', tool: 'Codex', title: 'Codex 日志',
    ...(updated(times?.codex_log, '记录')
      ? { status: updated(times?.codex_log, '记录')!, tone: 'ok', action: null }
      : { status: '暂无额度记录', tone: 'neutral', action: null }),
  } as SourceRow)
  return rows
}

/** The status button turns amber when any source needs attention or loading failed. */
export function overallTone(rows: SourceRow[], loadError: string): 'success' | 'warning' {
  return loadError || rows.some(row => row.tone === 'warning') ? 'warning' : 'success'
}
