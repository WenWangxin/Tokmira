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
  today_cache_read_share: number | null
  peak_hour: number | null
  hourly_tokens: number[]
  today_cache_hit_rate: number | null
  cache_hit_change_from_yesterday: number | null
  month_estimated_cost_usd: number
  daily_cost: { date: string; estimated_cost_usd: number }[]
  recorder: RecorderStatus
  trends: {
    tool: Tool
    five_hour: { at: string; percent: number }[]
    high_prompts: { at: string; delta: number; session_id: string; title: string | null }[]
  }[]
  recent_sessions: SessionOverview[]
  summary: { sessions: number; requests: number; high_prompts: number }
}

export type WindowKind = 'FiveHour' | 'Weekly'
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
export const rescan = () => invoke<ScanSummary>('rescan')
export const refreshQuotas = () => invoke<RefreshResult>('refresh_quotas')
export const getOverview = () => invoke<Overview>('get_overview')
export const getAttribution = () => invoke<Attribution>('get_attribution')
export const recorderStatus = () => invoke<RecorderStatus>('recorder_status')
export const recorderPlan = (parts: RecorderParts) => invoke<RecorderPlan>('recorder_plan', { parts })
export const recorderInstall = (parts: RecorderParts) => invoke<RecorderPlan>('recorder_install', { parts })
export const recorderUninstall = () => invoke<RecorderPlan>('recorder_uninstall')
