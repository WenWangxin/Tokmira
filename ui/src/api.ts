import { invoke } from '@tauri-apps/api/core'

export type Tool = 'Claude' | 'Codex'
export type Source = 'ClaudeCli' | 'ClaudeDesktop' | 'CodexCli' | 'CodexDesktop' | 'Unknown'
export type Origin = 'Official' | 'Statusline' | 'Log' | 'Manual' | 'Estimated'
export type Theme = 'system' | 'light' | 'dark'
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
  calibration: Calibration | null
  dismissed_advice: string[]
}

export interface QuotaDisplay {
  used_percent: number | null
  resets_at: string | null
  origin: Origin | null
  forecast: Forecast
  window_passed: boolean
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
export interface RecorderPlan {
  changes: {
    file: string
    key_path: string
    action: string
    before_command: string | null
    after_command: string | null
  }[]
  backup_path: string | null
  notes: string[]
}
export interface ScanSummary { events: number; prompts: number; malformed_lines: number }
export interface RefreshResult { claude: string; codex: string }

export const getSettings = () => invoke<AppSettings>('get_settings')
export const updateSettings = (settings: AppSettings) => invoke<AppSettings>('update_settings', { settings })
export const rescan = () =>
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? Promise.resolve({ events: 0, prompts: 0, malformed_lines: 0 })
    : invoke<ScanSummary>('rescan')
export const refreshQuotas = () =>
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? Promise.resolve({ claude: '开发预览', codex: '开发预览' })
    : invoke<RefreshResult>('refresh_quotas')
async function developmentOverview(): Promise<Overview> {
  const { mockOverview } = await import('./mocks/overview')
  const variant = new URLSearchParams(window.location.search).get('mock')
  if (variant === 'empty' || variant === 'nodata') return mockOverview(variant)
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
  if (variant === 'empty' || variant === 'nodata') return mockUsage(tool, variant)
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
export const recorderStatus = () => invoke<RecorderStatus>('recorder_status')
export const recorderPlan = (parts: RecorderParts) => invoke<RecorderPlan>('recorder_plan', { parts })
export const recorderInstall = (parts: RecorderParts) => invoke<RecorderPlan>('recorder_install', { parts })
export const recorderUninstall = () => invoke<RecorderPlan>('recorder_uninstall')

export type AdviceSeverity = 'Error' | 'Warning'
export interface AdviceLink { label: string; url: string }
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
}

const isPreview = () => import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
const adviceFiles = import.meta.glob<{ default: AdviceReport }>('../.local/advice-*.json')
const previewDismissed = new Set<string>()
async function developmentAdvice(periodDays: number): Promise<AdviceReport> {
  const { mockAdvice } = await import('./mocks/advice')
  const variant = new URLSearchParams(window.location.search).get('mock')
  const local = adviceFiles[`../.local/advice-${periodDays}.json`]
  const report = variant === 'empty' || variant === 'nodata' || !local ? mockAdvice(periodDays, variant) : (await local()).default
  // Mirror the backend: dismissed items are hidden and counted.
  return {
    ...report,
    tools: report.tools.map(tool => ({
      ...tool,
      items: tool.items.filter(item => !previewDismissed.has(item.key)),
      hidden: tool.hidden + tool.items.filter(item => previewDismissed.has(item.key)).length,
    })),
  }
}
export const getAdvice = (periodDays: 7 | 30) =>
  isPreview() ? developmentAdvice(periodDays) : invoke<AdviceReport>('get_advice', { periodDays })
export const dismissAdvice = async (key: string) => {
  if (isPreview()) { previewDismissed.add(key); return }
  await invoke<AppSettings>('dismiss_advice', { key })
}
export const restoreAdvice = async (key: string) => {
  if (isPreview()) { previewDismissed.delete(key); return }
  await invoke<AppSettings>('restore_advice', { key })
}
/** First diagnosis tag for a prompt, matched by tool, session and prompt time. */
export const promptTag = (tags: PromptTag[] | undefined, tool: Tool, sessionId: string, promptAt: string | null) =>
  promptAt ? tags?.find(tag => tag.tool === tool && tag.session_id === sessionId && Date.parse(tag.prompt_at) === Date.parse(promptAt)) : undefined
