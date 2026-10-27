export type ThemeChoice = 'system' | 'light' | 'dark'
export type QuotaMessageTone = 'ok' | 'neutral' | 'bad'
import type { QuotaOutcome } from './api'
import { m, monthDay } from './i18n/index.ts'

/**
 * Deep-copy a preview payload.
 * `structuredClone` throws DataCloneError on Vue reactive proxies; JSON walks the getters.
 */
export function clonePreview<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
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

/** `claude:cache_rebuild` → `Claude · 缓存过期后的整段重建`. */
export function adviceLabel(key: string): string {
  const separator = key.indexOf(':')
  const tool = separator === -1 ? key : key.slice(0, separator)
  const id = separator === -1 ? '' : key.slice(separator + 1)
  const toolName = tool === 'claude' ? 'Claude' : tool === 'codex' ? 'Codex' : tool
  const title = m().advice.titles[id] ?? (id || key)
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
  const stamp = `${monthDay(recordedAt)} ${String(recordedAt.getHours()).padStart(2, '0')}:${String(recordedAt.getMinutes()).padStart(2, '0')}`
  return m().settings.calibrationLine(tool, formatPercent(percent), stamp)
}

export function quotaMessageTone(outcome: QuotaOutcome): QuotaMessageTone {
  if (outcome.state === 'updated') return 'ok'
  return outcome.state === 'error' ? 'bad' : 'neutral'
}

export function quotaMessage(outcome: QuotaOutcome): string {
  return outcome.state === 'error' ? outcome.message : m().quotaState[outcome.state]
}
