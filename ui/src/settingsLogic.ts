export type ThemeChoice = 'system' | 'light' | 'dark'
export type QuotaMessageTone = 'ok' | 'neutral' | 'bad'

/**
 * Deep-copy a preview payload.
 * `structuredClone` throws DataCloneError on Vue reactive proxies; JSON walks the getters.
 */
export function clonePreview<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

const ADVICE_TITLES: Record<string, string> = {
  cache_rebuild: '缓存过期后的整段重建',
  session_startup: '新会话的启动开销',
  long_context: '上下文过长',
  expensive_light_task: '轻量任务使用了高价模型',
  many_subagents: '子代理使用过多',
}

/** `null` means the document should follow the system theme and drop `data-theme`. */
export function themeDataset(theme: ThemeChoice): 'light' | 'dark' | null {
  return theme === 'system' ? null : theme
}

export function applyTheme(theme: ThemeChoice, root: HTMLElement = document.documentElement) {
  const value = themeDataset(theme)
  if (value) root.dataset.theme = value
  else delete root.dataset.theme
}

/** Second sentence of the settings privacy banner. */
export function privacyDetail(claudeOfficial: boolean, codexOfficial: boolean): string {
  const enabled = [
    claudeOfficial ? 'Claude 官方额度查询' : '',
    codexOfficial ? 'Codex 官方额度' : '',
  ].filter(Boolean)
  if (!enabled.length) {
    return '仅读取 ~/.claude/projects 与 ~/.codex/sessions 中的日志，默认不发起任何网络请求；核心算法开源，结果可独立复现。'
  }
  return `已开启：${enabled.join(' / ')}，仅在刷新额度时联网。`
}

/** `claude:cache_rebuild` → `Claude · 缓存过期后的整段重建`. */
export function adviceLabel(key: string): string {
  const separator = key.indexOf(':')
  const tool = separator === -1 ? key : key.slice(0, separator)
  const id = separator === -1 ? '' : key.slice(separator + 1)
  const toolName = tool === 'claude' ? 'Claude' : tool === 'codex' ? 'Codex' : tool
  const title = ADVICE_TITLES[id] ?? (id || key)
  return `${toolName} · ${title}`
}

export function formatPercent(value: number): string {
  if (!Number.isFinite(value)) return ''
  const rounded = Math.round(value * 10) / 10
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1)
}

/** Accepts a 0–100 number. Blank or partial input is not a value yet. */
export function parsePercent(text: string): number | null {
  const trimmed = text.trim()
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null
  const value = Number(trimmed)
  if (!Number.isFinite(value) || value < 0 || value > 100) return null
  return value
}

export function formatCalibration(tool: string, percent: number, recordedAt: Date): string {
  const stamp = `${recordedAt.getMonth() + 1}/${recordedAt.getDate()} ${String(recordedAt.getHours()).padStart(2, '0')}:${String(recordedAt.getMinutes()).padStart(2, '0')}`
  return `上次校准：${tool} ${formatPercent(percent)}% · ${stamp}`
}

export function quotaMessageTone(message: string): QuotaMessageTone {
  if (message === '已更新') return 'ok'
  if (message === '未开启' || message === '60 秒内已查询' || message === '限流退避中' || message === '开发预览') return 'neutral'
  return 'bad'
}
