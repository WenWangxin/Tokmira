import { invoke } from '@tauri-apps/api/core'
import { mockDetectedTools } from './mocks/onboarding'
import type { ShareSummary } from './shareLogic'
import { mockEnvironment, mockRecorderStatus, mockSettings, previewRecorderPlan } from './mocks/settings'
import { clonePreview } from './settingsLogic'
import { noteQuotaResult } from './realtime'
import { locale } from './i18n'

const isPreview = () => import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
let previewSettings: AppSettings = clonePreview(mockSettings)
let previewRecorder: RecorderStatus = clonePreview(mockRecorderStatus)

export type Tool = 'Claude' | 'Codex'
export type Source = 'ClaudeCli' | 'ClaudeDesktop' | 'CodexCli' | 'CodexDesktop' | 'Unknown'
export type Origin = 'Official' | 'Statusline' | 'Log' | 'Manual' | 'Estimated'
export type Theme = 'system' | 'light' | 'dark'
export type Language = 'system' | 'zh' | 'en'
export type CredentialPreference = 'Auto' | 'ClaudeDesktop' | 'ClaudeCode'
export type Forecast = { WillHit: { at: string } } | 'Ample' | 'Unknown'

export interface TokenCounts {
  input: number
  output: number
  reasoning: number
  cache_read: number
  cache_write_5m: number
  cache_write_1h: number
  cache_write_unspecified: number
}

export interface Calibration {
  tool: Tool
  five_hour_percent: number
  recorded_at: string
}

export interface AppSettings {
  onboarding_completed: boolean
  claude_official_query: boolean
  claude_credential: CredentialPreference
  codex_app_server: boolean
  theme: Theme
  language: Language
  calibration: Calibration | null
  dismissed_advice: string[]
  advice_notifications: boolean
  tray_hint_shown: boolean
  silent_start: boolean
  /** Tools shown next to the menu bar icon, at most two; empty shows the icon only. */
  menu_bar_tools: Tool[]
  auto_check_updates: boolean
  /** Written by the backend only. */
  update_last_checked_at: string | null
  update_notified_version: string | null
}

/** A background reminder from the advice rules, sent while the window has focus. */
export interface AdviceAlert {
  key: string
  tool: Tool
  session_id: string
  title: string
  body: string
}

export interface QuotaDisplay {
  used_percent: number | null
  resets_at: string | null
  origin: Origin | null
  forecast: Forecast
  window_passed: boolean
  /** When the latest reading was taken. */
  as_of: string | null
  /** Estimated share of requests logged after the latest reading, already in `used_percent`. */
  estimated_percent: number | null
}

export interface ToolOverview {
  tool: Tool
  plan: string | null
  subscription_usd: number | null
  five_hour: QuotaDisplay
  weekly: QuotaDisplay
}

export interface RecorderStatus {
  statusline_installed: boolean
  statusline_modified: boolean
  hooks_installed: boolean
  hooks_modified: boolean
  statusline_records: number
  hook_events: number
  last_statusline_record: string | null
  last_hook_event: string | null
}

export interface SessionOverview {
  tool: Tool
  session_id: string
  title: string | null
  source: Source
  project: string | null
  model: string
  requests: number
  tokens: TokenCounts
  total_tokens: number
  cache_hit_rate: number | null
  five_hour_delta: number
  weekly_delta: number
  first_at: string
  last_at: string
}

export interface QuotaSourceTimes {
  claude_official: string | null
  codex_official: string | null
  codex_log: string | null
}

export interface Overview {
  generated_at: string
  tools: ToolOverview[]
  today_tokens: TokenCounts
  today_token_total: number
  today_cache_read_share: number | null
  peak_hour: number | null
  hourly_tokens: number[]
  today_cache_hit_rate: number | null
  daily_cache_hit_rate: { date: string; rate: number | null }[]
  cache_hit_change_from_yesterday: number | null
  month_estimated_cost_usd: number
  month_cost_by_tool: Partial<Record<Tool, number>>
  subscription_total_usd: number | null
  subscription_value_multiple: number | null
  daily_cost: { date: string; estimated_cost_usd: number }[]
  recorder: RecorderStatus
  /** When each quota source last delivered data. */
  quota_sources: QuotaSourceTimes
  trends: {
    tool: Tool
    five_hour_started_at: string | null
    five_hour: { at: string; percent: number }[]
    high_prompts: { at: string; delta: number; session_id: string; title: string | null }[]
    today: { at: string; percent: number }[]
    today_high_prompts: { at: string; delta: number; session_id: string; title: string | null }[]
    week_started_at: string | null
    week: { at: string; percent: number }[]
    week_high_prompts: { at: string; delta: number; session_id: string; title: string | null }[]
  }[]
  recent_sessions: SessionOverview[]
  recent_high_prompts: (Omit<SessionOverview, 'requests' | 'first_at' | 'last_at'> & { at: string })[]
  summary: { sessions: number; requests: number; high_prompts: number }
  /** `[quota_hit] threshold_pct` from `config/advice.toml`; older caches lack it. */
  quota_hit_threshold_pct?: number | null
}

export type WindowKind = 'FiveHour' | 'Weekly'
export interface UsageWindowRef {
  tool: Tool
  kind: WindowKind
  window_id: string
  start: string
  end: string
  is_current: boolean
}
export interface UsageRank {
  key: string
  name: string
  subtitle: string
  tokens: number
  requests: number
  delta: number
}
export interface UsagePrompt {
  at: string
  session_id: string
  title: string
  delta: number
  requests: number
  tokens: number
}
export interface UsageDetail {
  window: UsageWindowRef
  used_percent: number
  remaining_percent: number
  resets_at: string
  origin: Origin
  attributed_total: number
  unattributed_total: number
  sessions: UsageRank[]
  projects: UsageRank[]
  models: UsageRank[]
  subagents: UsageRank[]
  high_prompts: UsagePrompt[]
  official_weekly_breakdown: Attribution['latest_claude_weekly_breakdown']
}
export interface UsageResponse { windows: UsageWindowRef[]; detail: UsageDetail | null }
export interface UsageEvent {
  tool: Tool
  source: Source
  session_id: string
  request_id: string | null
  timestamp: string
  model: string
  project: string | null
  is_subagent: boolean
  tokens: TokenCounts
}
export interface EventAttribution {
  event: UsageEvent
  five_hour_delta: number
  weekly_delta: number
}
export interface WindowAttribution {
  tool: Tool
  kind: WindowKind
  window_id: string
  points: number
  increment_total: number
  attributed_total: number
  unattributed_total: number
  decreases: number
  by_session: { key: string; delta: number }[]
  by_project: { key: string; delta: number }[]
  by_model: { key: string; delta: number }[]
  by_source: { key: string; delta: number }[]
  by_subagent: { key: string; delta: number }[]
}
export interface PromptAttribution {
  tool: Tool
  session_id: string
  title: string | null
  prompt_at: string | null
  first_at: string
  last_at: string
  requests: number
  tokens: TokenCounts
  five_hour_delta: number
  weekly_delta: number
}
export interface WindowPromptAttribution extends Omit<PromptAttribution, 'five_hour_delta' | 'weekly_delta'> {
  kind: WindowKind
  window_id: string
  delta: number
}
export interface Attribution {
  events: EventAttribution[]
  windows: WindowAttribution[]
  prompts: PromptAttribution[]
  window_prompts: WindowPromptAttribution[]
  latest_claude_weekly_breakdown: {
    as_of: string | null
    window_started_at: string | null
    rows: { key: string; display_name: string | null; percent: number }[]
  } | null
}

export interface RecorderParts { statusline: boolean; hooks: boolean }
export type RecorderAction = 'add' | 'update' | 'replace' | 'restore' | 'remove'
export type RecorderNote = 'statusline_backup_missing' | 'statusline_modified'
export interface RecorderPlan {
  changes: {
    file: string
    key_path: string
    action: RecorderAction
    before_command: string | null
    after_command: string | null
  }[]
  backup_path: string | null
  notes: RecorderNote[]
}
export interface ScanSummary { events: number; prompts: number; malformed_lines: number }
/** What happened to one official quota query; `error` carries the backend's message. */
export type QuotaState = 'off' | 'updated' | 'needs_authorization' | 'backoff' | 'recent' | 'error' | 'preview'
export interface QuotaOutcome { state: QuotaState; message: string }
export interface RefreshResult { claude: QuotaOutcome; codex: QuotaOutcome }
export interface DirectoryStatus { path: string; exists: boolean }
export interface DetectedTool {
  source: Source
  sessions: number
  requests: number
  last_used_at: string | null
  plan: string | null
}

export interface EnvironmentInfo {
  claude_logs: DirectoryStatus
  codex_logs: DirectoryStatus
  data_dir: string
  pricing_version: string
  pricing_note: string
  advice_version: string
  backup_dir: string
}

export const getSettings = () =>
  isPreview() ? Promise.resolve(clonePreview(previewSettings)) : invoke<AppSettings>('get_settings')
export const detectTools = () =>
  isPreview()
    ? Promise.resolve(clonePreview(mockDetectedTools(new URLSearchParams(window.location.search).get('mock'))))
    : invoke<DetectedTool[]>('detect_tools')
export const updateSettings = (settings: AppSettings) => {
  if (isPreview()) {
    const percent = settings.calibration?.five_hour_percent
    if (percent !== undefined && (!Number.isFinite(percent) || percent < 0 || percent > 100)) {
      return Promise.reject(new Error('校准百分比应在 0 到 100 之间'))
    }
    previewSettings = clonePreview(settings)
    return Promise.resolve(clonePreview(previewSettings))
  }
  return invoke<AppSettings>('update_settings', { settings })
}
export const getEnvironment = () =>
  isPreview() ? Promise.resolve(clonePreview(mockEnvironment)) : invoke<EnvironmentInfo>('get_environment')
export const rescan = () =>
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? Promise.resolve({ events: 0, prompts: 0, malformed_lines: 0 })
    : invoke<ScanSummary>('rescan')
/**
 * `automatic` marks background polls; the backend then never re-prompts for a keychain the user dismissed.
 * Every result goes through {@link noteQuotaResult}, so a success from any page clears “需要授权”.
 */
export async function refreshQuotas(automatic = false): Promise<RefreshResult> {
  const result = isPreview()
    ? {
        claude: { state: previewSettings.claude_official_query ? 'preview' as const : 'off' as const, message: '' },
        codex: { state: previewSettings.codex_app_server ? 'preview' as const : 'off' as const, message: '' },
      }
    : await invoke<RefreshResult>('refresh_quotas', { automatic })
  noteQuotaResult(result)
  return result
}
async function developmentOverview(): Promise<Overview> {
  const { mockOverview } = await import('./mocks/overview')
  const variant = new URLSearchParams(window.location.search).get('mock')
  if (variant === 'empty' || variant === 'nodata' || variant === 'hit' || variant === 'hit-estimated') return mockOverview(variant)
  if (variant === 'demo') return mockOverview('default')
  const files = import.meta.glob<{ default: Overview }>('../.local/overview.json')
  const local = files['../.local/overview.json']
  return local ? (await local()).default : mockOverview('default')
}
export const getOverview = () =>
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? developmentOverview()
    : invoke<Overview>('get_overview')
export const getAttribution = () => invoke<Attribution>('get_attribution')
const usageFiles = import.meta.glob<{ default: UsageResponse }>('../.local/usage-*.json')
async function developmentUsage(tool: Tool): Promise<UsageResponse> {
  const { mockUsage } = await import('./mocks/usage')
  const variant = new URLSearchParams(window.location.search).get('mock')
  if (variant === 'empty' || variant === 'nodata' || variant === 'unattributed') return mockUsage(tool, variant)
  if (variant === 'hit' || variant === 'hit-estimated') return mockUsage(tool, 'default')
  const local = usageFiles[`../.local/usage-${tool.toLowerCase()}.json`]
  return local ? (await local()).default : mockUsage(tool, 'default')
}
export const getUsageWindows = async (tool: Tool) =>
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? (await developmentUsage(tool)).windows
    : invoke<UsageWindowRef[]>('get_usage_windows', { tool })
export const getUsageDetail = async (tool: Tool, kind: WindowKind, windowId: string) => {
  if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
    const response = await developmentUsage(tool)
    if (response.detail?.window.kind === kind && response.detail.window.window_id === windowId) return response.detail
    const { mockUsageDetail } = await import('./mocks/usage')
    if (new URLSearchParams(window.location.search).get('mock') || !usageFiles[`../.local/usage-${tool.toLowerCase()}.json`]) {
      const selected = response.windows.find(item => item.kind === kind && item.window_id === windowId)
      if (selected) return mockUsageDetail(selected)
    }
    throw new Error('本地预览文件仅包含一个窗口；请用 CLI 为所选窗口重新生成 JSON')
  }
  return invoke<UsageDetail>('get_usage_detail', { tool, kind, windowId })
}
function applyPreviewRecorder(parts: RecorderParts, action: 'install' | 'uninstall') {
  if (parts.statusline) {
    previewRecorder.statusline_installed = action === 'install'
    previewRecorder.statusline_modified = false
  }
  if (parts.hooks) {
    previewRecorder.hooks_installed = action === 'install'
    previewRecorder.hooks_modified = false
  }
  return previewRecorderPlan(parts, action)
}
/** macOS reports the real permission; Windows and the dev build cannot read it (`unknown`). */
export type NotificationStatus = 'unknown' | 'not_determined' | 'denied' | 'authorized'
export interface AutostartState {
  enabled: boolean
  /** False in the dev build, where a login item would point at the debug binary. */
  available: boolean
  requires_approval: boolean
}
let previewNotifications: NotificationStatus = 'not_determined'
let previewAutostart: AutostartState = { enabled: false, available: true, requires_approval: false }
export const notificationStatus = () =>
  isPreview() ? Promise.resolve(previewNotifications) : invoke<NotificationStatus>('notification_status')
/** Shows the system permission prompt when the user has not answered it yet. */
export const requestNotifications = () => {
  if (isPreview()) {
    if (previewNotifications === 'not_determined') previewNotifications = 'authorized'
    return Promise.resolve(previewNotifications)
  }
  return invoke<NotificationStatus>('request_notifications')
}
export const getAutostart = () =>
  isPreview() ? Promise.resolve({ ...previewAutostart }) : invoke<AutostartState>('get_autostart')
export const setAutostart = (enabled: boolean) => {
  if (isPreview()) {
    previewAutostart = { ...previewAutostart, enabled }
    return Promise.resolve({ ...previewAutostart })
  }
  return invoke<AutostartState>('set_autostart', { enabled })
}
export const openLoginItemsSettings = () =>
  isPreview() ? Promise.resolve() : invoke<void>('open_login_items_settings')

export interface UpdateInfo {
  version: string
  notes: string | null
  date: string | null
}
export interface UpdateStatus {
  current_version: string
  last_checked_at: string | null
  available: UpdateInfo | null
  /** The new version is in place; only a restart is left. */
  installed: boolean
}
export interface UpdateProgress {
  downloaded: number
  total: number | null
}
// The browser preview finds a made-up 0.2.0 so the dialog can be tried without GitHub.
let previewUpdate: UpdateStatus = { current_version: '0.1.0', last_checked_at: null, available: null, installed: false }
const previewRelease: UpdateInfo = { version: '0.2.0', notes: '仅供开发预览的更新说明。\n- 新增检查更新\n- 修复若干问题', date: null }
export const getUpdateStatus = () =>
  isPreview() ? Promise.resolve(clonePreview(previewUpdate)) : invoke<UpdateStatus>('get_update_status')
export const checkUpdate = () => {
  if (isPreview()) {
    const latest = new URLSearchParams(window.location.search).get('mock') === 'latest'
    previewUpdate = { ...previewUpdate, last_checked_at: new Date().toISOString(), available: latest ? null : previewRelease }
    return new Promise<UpdateStatus>(resolve => setTimeout(() => resolve(clonePreview(previewUpdate)), 600))
  }
  return invoke<UpdateStatus>('check_update')
}
export const installUpdate = () => {
  if (isPreview()) {
    previewUpdate = { ...previewUpdate, installed: true }
    return new Promise<void>(resolve => setTimeout(resolve, 1200))
  }
  return invoke<void>('install_update')
}
export const restartApp = () => (isPreview() ? Promise.resolve() : invoke<void>('restart_app'))

export const recorderStatus = () =>
  isPreview() ? Promise.resolve(clonePreview(previewRecorder)) : invoke<RecorderStatus>('recorder_status')
export const recorderPlan = (parts: RecorderParts) =>
  isPreview() ? Promise.resolve(previewRecorderPlan(parts, 'install')) : invoke<RecorderPlan>('recorder_plan', { parts })
export const recorderPlanUninstall = (parts: RecorderParts) =>
  isPreview() ? Promise.resolve(previewRecorderPlan(parts, 'uninstall')) : invoke<RecorderPlan>('recorder_plan_uninstall', { parts })
export const recorderInstall = (parts: RecorderParts) =>
  isPreview() ? Promise.resolve(applyPreviewRecorder(parts, 'install')) : invoke<RecorderPlan>('recorder_install', { parts })
export const recorderUninstall = (parts: RecorderParts) =>
  isPreview() ? Promise.resolve(applyPreviewRecorder(parts, 'uninstall')) : invoke<RecorderPlan>('recorder_uninstall', { parts })
export const clearRecorderRecords = () => {
  if (isPreview()) {
    previewRecorder.statusline_records = 0
    previewRecorder.hook_events = 0
    previewRecorder.last_statusline_record = null
    previewRecorder.last_hook_event = null
    return Promise.resolve()
  }
  return invoke<void>('clear_recorder_records')
}

export type AdviceSeverity = 'Error' | 'Warning'
export interface AdviceLink { label: string; url: string }
/** `command` is null unless verified for that tool; the copy button is shown only with a command. */
export interface NextStep { text: string; command: string | null }
export interface AdviceItem {
  id: string
  key: string
  tool: Tool
  title: string
  severity: AdviceSeverity
  facts: string[]
  description: string
  occurrences: number
  sessions: number
  overhead_tokens: number
  overhead_cost_usd: number
  consumed_weekly_pct: number | null
  avoidable: boolean
  avoidable_cost_usd: number
  avoidable_weekly_pct: number | null
  link: AdviceLink | null
  next_step: NextStep | null
}
export interface ToolAdvice {
  tool: Tool
  has_quota_data: boolean
  summary: { avoidable_weekly_pct: number | null; avoidable_cost_usd: number; count: number; sessions: number }
  items: AdviceItem[]
  hidden: number
}
export interface PromptTag { tool: Tool; session_id: string; prompt_at: string; tag: string; severity: AdviceSeverity }
export interface AdviceReport {
  generated_at: string
  period_days: number
  config_version: string
  tools: ToolAdvice[]
  prompt_tags: PromptTag[]
  event_tags: EventTag[]
}
export interface EventTag {
  tool: Tool
  session_id: string
  request_id: string | null
  at: string
  tag: string
  severity: AdviceSeverity
}

const adviceFiles = import.meta.glob<{ default: AdviceReport }>('../.local/advice-*.json')
async function developmentAdvice(periodDays: number): Promise<AdviceReport> {
  const { mockAdvice } = await import('./mocks/advice')
  const variant = new URLSearchParams(window.location.search).get('mock')
  const local = adviceFiles[`../.local/advice-${periodDays}.json`]
  // Local snapshots predate localization; use the synthetic bilingual cases in English.
  const report = variant === 'demo' || variant === 'empty' || variant === 'nodata' || variant === 'hit' || variant === 'hit-estimated' || !local || locale.value === 'en' ? mockAdvice(periodDays, variant) : (await local()).default
  const dismissed = new Set(previewSettings.dismissed_advice)
  // Mirror the backend: dismissed items are hidden and counted.
  return {
    ...report,
    event_tags: report.event_tags ?? [],
    tools: report.tools.map(tool => ({
      ...tool,
      items: tool.items.filter(item => !dismissed.has(item.key)),
      hidden: tool.hidden + tool.items.filter(item => dismissed.has(item.key)).length,
    })),
  }
}
export const getAdvice = (periodDays: 7 | 30) =>
  isPreview() ? developmentAdvice(periodDays) : invoke<AdviceReport>('get_advice', { periodDays })
export const dismissAdvice = async (key: string) => {
  if (isPreview()) {
    if (!previewSettings.dismissed_advice.includes(key)) previewSettings.dismissed_advice.push(key)
    return
  }
  await invoke<AppSettings>('dismiss_advice', { key })
}
export const restoreAdvice = async (key: string) => {
  if (isPreview()) {
    previewSettings.dismissed_advice = previewSettings.dismissed_advice.filter(item => item !== key)
    return
  }
  await invoke<AppSettings>('restore_advice', { key })
}
/** First diagnosis tag for a prompt, matched by tool, session and prompt time. */
export const promptTag = (tags: PromptTag[] | undefined, tool: Tool, sessionId: string, promptAt: string | null) =>
  promptAt ? tags?.find(tag => tag.tool === tool && tag.session_id === sessionId && Date.parse(tag.prompt_at) === Date.parse(promptAt)) : undefined

export interface SessionRow {
  tool: Tool
  session_id: string
  title: string | null
  display_name: string
  project: string | null
  project_name: string
  source: Source
  model: string
  requests: number
  total_tokens: number
  cache_hit_rate: number | null
  estimated_cost_usd: number
  weekly_delta: number
  first_at: string
  last_at: string
  has_quota_data: boolean
}
export interface TimelineTag { tag: string; severity: AdviceSeverity }
export type TimelineEntry =
  | { kind: 'Prompt'; at: string; gap_minutes_from_previous_prompt: number | null }
  | {
      kind: 'Request'
      at: string
      index: number
      is_subagent: boolean
      model: string
      tokens: TokenCounts
      weekly_delta: number
      tag: TimelineTag | null
    }
export interface SessionDetail {
  tool: Tool
  session_id: string
  title: string | null
  display_name: string
  project: string | null
  project_name: string
  source: Source
  model: string
  requests: number
  total_tokens: number
  cache_hit_rate: number | null
  estimated_cost_usd: number
  weekly_delta: number
  has_quota_data: boolean
  first_at: string
  last_at: string
  entries: TimelineEntry[]
}

const sessionListFiles = import.meta.glob<{ default: SessionRow[] }>('../.local/sessions.json')
const sessionDetailFiles = import.meta.glob<{ default: SessionDetail }>('../.local/session-*.json')
async function developmentSessions(): Promise<SessionRow[]> {
  const { mockSessions } = await import('./mocks/sessions')
  const variant = new URLSearchParams(window.location.search).get('mock')
  if (variant === 'empty' || variant === 'nodata') return []
  if (variant === 'demo') return mockSessions()
  const local = sessionListFiles['../.local/sessions.json']
  return local ? (await local()).default : mockSessions()
}
async function developmentSessionDetail(tool: Tool, sessionId: string): Promise<SessionDetail> {
  const variant = new URLSearchParams(window.location.search).get('mock')
  if (variant === 'empty' || variant === 'nodata') throw new Error('未找到该会话')
  const file = sessionDetailFiles[`../.local/session-${sessionId}.json`]
  if (file && variant !== 'demo') return (await file()).default
  if (sessionListFiles['../.local/sessions.json'] && variant !== 'demo') {
    throw new Error('浏览器预览没有这份会话详情。请用 token-audit session --json 生成 ui/.local/session-<id>.json')
  }
  const { mockSessionDetail } = await import('./mocks/sessions')
  const detail = mockSessionDetail(tool, sessionId)
  if (!detail) throw new Error('未找到该会话')
  return detail
}
export const getSessions = (days: 7 | 30 = 7) =>
  isPreview() ? developmentSessions() : invoke<SessionRow[]>('get_sessions', { days })
export const getSessionDetail = (tool: Tool, sessionId: string) =>
  isPreview()
    ? developmentSessionDetail(tool, sessionId)
    : invoke<SessionDetail>('get_session_detail', { tool, sessionId })

const shareFiles = import.meta.glob<{ default: ShareSummary }>('../.local/share.json')
async function developmentShare(): Promise<ShareSummary> {
  const local = shareFiles['../.local/share.json']
  if (local) return (await local()).default
  const { mockShareSummary } = await import('./mocks/share')
  return mockShareSummary()
}
function previewDownload(fileName: string, bytes: Uint8Array): string {
  const mime = fileName.endsWith('.md') ? 'text/markdown;charset=utf-8' : 'image/png'
  const body = new ArrayBuffer(bytes.byteLength)
  new Uint8Array(body).set(bytes)
  const blob = new Blob([body], { type: mime })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = fileName
  document.body.append(link)
  link.click()
  link.remove()
  URL.revokeObjectURL(url)
  return fileName
}
export const getShareSummary = () =>
  isPreview() ? developmentShare() : invoke<ShareSummary>('get_share_summary')
export const saveExport = (fileName: string, bytes: Uint8Array) =>
  isPreview()
    ? Promise.resolve(previewDownload(fileName, bytes))
    : invoke<string | null>('save_export', { fileName, bytes: Array.from(bytes) })
