import { quotaMessageTone, quotaMessage } from './settingsLogic.ts'
import { m } from './i18n/index.ts'
import type { RecorderNote } from './api'
import type { QuotaOutcome, RefreshResult } from './api'

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

export function toolSources(): readonly ToolSourceInfo[] {
  const t = m().onboarding
  return [
    { source: 'ClaudeCli', name: t.claudeCli, logo: 'claude', quotaFromLogs: false, quotaHint: t.claudeCliHint },
    { source: 'ClaudeDesktop', name: t.claudeDesktop, logo: 'claude', quotaFromLogs: false },
    { source: 'CodexCli', name: t.codexCli, logo: 'codex', quotaFromLogs: true },
    { source: 'CodexDesktop', name: t.codexDesktop, logo: 'codex', quotaFromLogs: true },
  ]
}

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
export function detectedToolRows(detected: readonly DetectedSource[], registry: readonly ToolSourceInfo[] = toolSources()): ToolRow[] {
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
  if (sessions <= 0) return m().onboarding.noLogs
  const count = m().onboarding.sessions(sessions)
  const name = planLabel(source, plan)
  return name ? `${name} · ${count}` : count
}

export interface PrecisionPill {
  text: string
  tone: 'success' | 'neutral'
}

export function precisionPill(source: string, sessions: number, registry: readonly ToolSourceInfo[] = toolSources()): PrecisionPill {
  if (sessions <= 0) return { text: m().onboarding.notDetected, tone: 'neutral' }
  return sourceInfo(source, registry).quotaFromLogs ? { text: m().onboarding.tokenAndQuota, tone: 'success' } : { text: m().onboarding.tokenExact, tone: 'success' }
}

/** One quota source in step 3. Add a row here (plus its setting) to offer a new one. */
export interface QuotaSourceInfo {
  key: keyof OnboardingSelection
  title: string
  description: string
  /** Shown under the description, e.g. a system prompt the user will see. */
  hint?: string
}

export function quotaSources(): readonly QuotaSourceInfo[] {
  const t = m().onboarding
  return [
    { key: 'statusline', title: t.statuslineTitle, description: t.statuslineDesc },
    { key: 'claudeOfficial', title: t.claudeTitle, description: t.claudeDesc, hint: t.claudeHint },
    { key: 'codexOfficial', title: t.codexTitle, description: t.codexDesc },
  ]
}

export function installOutcome(backupPath: string | null, notes: RecorderNote[]): string[] {
  if (backupPath) return [m().settings.backedUp(backupPath)]
  if (notes.length) return notes.map(note => m().recorder.note[note])
  return [m().settings.noFileChanged]
}

export interface QuotaLine { text: string; outcome: QuotaOutcome }
export function quotaOutcome(
  selection: OnboardingSelection,
  result: RefreshResult,
): QuotaLine[] {
  const lines: QuotaLine[] = []
  if (selection.claudeOfficial) lines.push({ text: m().onboarding.claudeOutcome(quotaMessage(result.claude)), outcome: result.claude })
  if (selection.codexOfficial) lines.push({ text: m().onboarding.codexOutcome(quotaMessage(result.codex)), outcome: result.codex })
  return lines
}

/** Green when every quota outcome is expected; amber when a query reported an error. */
export function outcomeTone(lines: readonly QuotaLine[]): 'success' | 'warning' {
  return lines.some(line => quotaMessageTone(line.outcome) === 'bad') ? 'warning' : 'success'
}

/** The message shown after setup: short when all went well, only the failures otherwise. */
export function onboardingSummary(lines: readonly QuotaLine[], operationErrors: readonly string[] = []): { text: string; tone: 'success' | 'warning' } {
  const failures = [...operationErrors, ...lines.filter(line => quotaMessageTone(line.outcome) === 'bad').map(line => line.text)]
  return failures.length ? { text: failures.join(m().onboarding.failureSeparator), tone: 'warning' } : { text: m().onboarding.done, tone: 'success' }
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
  if (status === 'authorized') return { text: m().notifications.authorized, tone: 'success' }
  if (status === 'denied') return { text: m().notifications.denied, tone: 'warning' }
  if (status === 'not_determined') return { text: m().notifications.notDetermined, tone: 'neutral' }
  return { text: m().notifications.unknown, tone: 'neutral' }
}

/** Where the app stays after its window closes. */
export function trayPlace(userAgent: string): string {
  return /Mac/.test(userAgent) ? m().notifications.menuBar : m().notifications.taskbar
}
