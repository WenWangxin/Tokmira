import { quotaMessageTone } from './settingsLogic.ts'

export interface OnboardingSelection {
  statusline: boolean
  claudeOfficial: boolean
  codexOfficial: boolean
}

export interface OnboardingSettingsFlags {
  claude_official_query: boolean
  codex_app_server: boolean
}

/** First run uses the guide defaults. Reopening keeps the switches the user already chose. */
export function initialOnboardingChoices(
  onboardingCompleted: boolean,
  settings: OnboardingSettingsFlags,
  statuslineOn: boolean,
): OnboardingSelection {
  if (!onboardingCompleted) return { statusline: true, claudeOfficial: true, codexOfficial: true }
  return {
    statusline: statuslineOn,
    claudeOfficial: settings.claude_official_query,
    codexOfficial: settings.codex_app_server,
  }
}

const CLAUDE_PLANS: Record<string, string> = {
  pro: 'Claude Pro',
  max: 'Claude Max',
  max_5x: 'Claude Max 5x',
  max_20x: 'Claude Max 20x',
}

const CODEX_PLANS: Record<string, string> = {
  plus: 'ChatGPT Plus',
  pro: 'ChatGPT Pro',
}

/** Display name for a stored plan. Blank plans are omitted. */
export function planLabel(source: string, plan: string | null): string | null {
  if (!plan) return null
  const trimmed = plan.trim()
  if (!trimmed) return null
  const key = trimmed.toLowerCase().replace(/[\s-]+/g, '_')
  const known: Record<string, string> = source.startsWith('Claude') ? CLAUDE_PLANS : source.startsWith('Codex') ? CODEX_PLANS : {}
  return known[key] ?? trimmed
}

export type ToolLogo = 'claude' | 'codex' | 'generic'

/** One local log source shown in step 2. Add a row here to support a new tool. */
export interface ToolSourceInfo {
  source: string
  name: string
  logo: ToolLogo
  /** Whether the logs alone give exact quota percentages. */
  quotaFromLogs: boolean
  /** Extra line when the source has logs but needs a quota source from step 3. */
  quotaHint?: string
}

export const TOOL_SOURCES: readonly ToolSourceInfo[] = [
  { source: 'ClaudeCli', name: 'Claude Code · 命令行', logo: 'claude', quotaFromLogs: false, quotaHint: '额度需开启记录器或官方查询' },
  { source: 'ClaudeDesktop', name: 'Claude · 桌面版', logo: 'claude', quotaFromLogs: false },
  { source: 'CodexCli', name: 'Codex · 命令行', logo: 'codex', quotaFromLogs: true },
  { source: 'CodexDesktop', name: 'Codex · 桌面版', logo: 'codex', quotaFromLogs: true },
]

export interface DetectedSource {
  source: string
  sessions: number
  requests: number
  plan: string | null
}

export interface ToolRow {
  source: string
  name: string
  logo: ToolLogo
  subtitle: string
  hint: string | null
  pill: PrecisionPill
}

function sourceInfo(source: string, registry: readonly ToolSourceInfo[]): ToolSourceInfo {
  return registry.find(item => item.source === source) ?? { source, name: source, logo: 'generic', quotaFromLogs: false }
}

/** Registry rows in order, then any detected source the registry does not know yet. */
export function detectedToolRows(detected: readonly DetectedSource[], registry: readonly ToolSourceInfo[] = TOOL_SOURCES): ToolRow[] {
  const extra = detected.filter(item => item.source !== 'Unknown' && !registry.some(info => info.source === item.source))
  return [...registry, ...extra.map(item => sourceInfo(item.source, registry))].map(info => {
    const found = detected.find(item => item.source === info.source)
    const sessions = found?.sessions ?? 0
    return {
      source: info.source,
      name: info.name,
      logo: info.logo,
      subtitle: toolSubtitle(info.source, sessions, found?.plan ?? null),
      hint: sessions > 0 ? info.quotaHint ?? null : null,
      pill: precisionPill(info.source, sessions, registry),
    }
  })
}

/** Subtitle under a detected tool. No sessions means this source has no local log. */
export function toolSubtitle(source: string, sessions: number, plan: string | null): string {
  if (sessions <= 0) return '未检测到本机日志'
  const count = `${sessions} 个会话`
  const name = planLabel(source, plan)
  return name ? `${name} · ${count}` : count
}

export interface PrecisionPill {
  text: string
  tone: 'success' | 'neutral'
}

export function precisionPill(source: string, sessions: number, registry: readonly ToolSourceInfo[] = TOOL_SOURCES): PrecisionPill {
  if (sessions <= 0) return { text: '未检测到', tone: 'neutral' }
  return sourceInfo(source, registry).quotaFromLogs ? { text: 'token 与额度精确', tone: 'success' } : { text: 'token 精确', tone: 'success' }
}

/** One quota source in step 3. Add a row here (plus its setting) to offer a new one. */
export interface QuotaSourceInfo {
  key: keyof OnboardingSelection
  title: string
  description: string
  /** Shown under the description, e.g. a system prompt the user will see. */
  hint?: string
}

export const QUOTA_SOURCES: readonly QuotaSourceInfo[] = [
  { key: 'statusline', title: 'Claude Code 状态栏记录', description: '不联网。从 Claude Code 状态栏读取官方额度。' },
  { key: 'claudeOfficial', title: 'Claude 官方额度查询', description: '联网。向 Anthropic 查询额度，令牌不保存。', hint: '首次查询时系统可能要求输入登录密码' },
  { key: 'codexOfficial', title: 'Codex 官方额度', description: '联网。通过本机 Codex 查询额度，不读取你的令牌。' },
]

export function installOutcome(backupPath: string | null, notes: string[]): string[] {
  if (backupPath) return [`已备份到 ${backupPath}`]
  if (notes.length) return [...notes]
  return ['没有修改配置文件']
}

export function quotaOutcome(
  selection: OnboardingSelection,
  result: { claude: string; codex: string },
): string[] {
  const lines: string[] = []
  if (selection.claudeOfficial) lines.push(`Claude 官方额度：${result.claude}`)
  if (selection.codexOfficial) lines.push(`Codex 官方额度：${result.codex}`)
  return lines
}

/** Green when every quota sentence is an expected status; amber when a query reported an error. */
export function outcomeTone(lines: string[]): 'success' | 'warning' {
  const bad = lines.some(line => {
    const separator = line.lastIndexOf('：')
    if (separator === -1) return false
    return quotaMessageTone(line.slice(separator + 1)) === 'bad'
  })
  return bad ? 'warning' : 'success'
}

/** The message shown after setup: short when all went well, only the failures otherwise. */
export function onboardingSummary(lines: string[]): { text: string; tone: 'success' | 'warning' } {
  const failures = lines.filter(line => {
    const separator = line.lastIndexOf('：')
    return separator !== -1 && quotaMessageTone(line.slice(separator + 1)) === 'bad'
  })
  return failures.length ? { text: failures.join('；'), tone: 'warning' } : { text: '设置完成，额度会自动更新', tone: 'success' }
}

/** Browser preview only. Packaged and Tauri sessions follow the saved setting. */
export function previewOnboardingForced(search: string, isPreview: boolean): boolean {
  if (!isPreview) return false
  return new URLSearchParams(search).get('onboarding') === '1'
}

export type NotificationState = 'unknown' | 'not_determined' | 'denied' | 'authorized'

export interface NotificationLine {
  text: string
  tone: 'success' | 'warning' | 'neutral'
}

/** What the reminder step and the settings page say about system notifications. */
export function notificationLine(status: NotificationState): NotificationLine {
  if (status === 'authorized') return { text: '已允许发送通知', tone: 'success' }
  if (status === 'denied') return { text: '未允许，可在系统设置 → 通知中开启', tone: 'warning' }
  if (status === 'not_determined') return { text: '尚未允许发送通知', tone: 'neutral' }
  return { text: '无法读取通知权限', tone: 'neutral' }
}

/** Where the app stays after its window closes. */
export function trayPlace(userAgent: string): string {
  return /Mac/.test(userAgent) ? '菜单栏' : '任务栏右下角'
}
