import { quotaMessageTone, quotaMessage } from './settingsLogic.ts'
import { m } from './i18n/index.ts'
import type { RecorderNote } from './api'
import type { QuotaOutcome, RefreshResult } from './api'

export interface OnboardingSelection {
  statusline: boolean
  claudeOfficial: boolean
  codexOfficial: boolean
  deepseekOfficial: boolean
}

export interface OnboardingSettingsFlags {
  claude_official_query: boolean
  codex_app_server: boolean
  deepseek_balance_query?: boolean
}

/**
 * First run uses the guide defaults: the status line (local only) is on, official queries stay off
 * until the user turns them on. Reopening keeps the switches the user already chose.
 */
export function initialOnboardingChoices(
  onboardingCompleted: boolean,
  settings: OnboardingSettingsFlags,
  statuslineOn: boolean,
): OnboardingSelection {
  if (!onboardingCompleted) return { statusline: true, claudeOfficial: false, codexOfficial: false, deepseekOfficial: false }
  return {
    statusline: statuslineOn,
    claudeOfficial: settings.claude_official_query,
    codexOfficial: settings.codex_app_server,
    deepseekOfficial: Boolean(settings.deepseek_balance_query),
  }
}

export interface DetectedSource {
  source: string
  sessions: number
  requests: number
  plan: string | null
  /** DeepSeek Harness only: its logs do not say which of its apps made a session. */
  install?: { desktop: boolean; cli: boolean } | null
}

/**
 * What step 2 found. `none`: no source has a local log. `claude_desktop_only`: Claude logs come only
 * from the desktop app, which never runs the status line. Everything else is `normal`.
 */
export type DetectionCase = 'none' | 'claude_desktop_only' | 'normal'

export function detectionCase(detected: readonly DetectedSource[]): DetectionCase {
  const sessions = (source: string) => detected.find(item => item.source === source)?.sessions ?? 0
  if (!detected.some(item => item.sessions > 0)) return 'none'
  if (sessions('ClaudeDesktop') > 0 && sessions('ClaudeCli') <= 0) return 'claude_desktop_only'
  return 'normal'
}

/** One app of a tool in step 2: a green dot when found, grey when not. */
export interface ToolVariant {
  label: string
  found: boolean
  detail: string
}

/** One tool in step 2, its command line and desktop apps side by side. */
export interface ToolGroup {
  tool: 'Claude' | 'Codex' | 'DeepSeek'
  name: string
  logo: 'claude' | 'codex' | 'deepseek'
  variants: [ToolVariant, ToolVariant]
  hint: string | null
  pill: PrecisionPill
}

function variant(label: string, found: boolean, sessions: number): ToolVariant {
  const t = m().onboarding
  return { label, found, detail: !found ? t.notDetected : sessions > 0 ? t.sessions(sessions) : t.installed }
}

/**
 * Claude and Codex count each app by its own logs. DeepSeek Harness writes one log for
 * both, so its dots come from what is installed and its sessions go with the desktop app
 * when that is installed, otherwise with the command line.
 */
export function toolGroups(detected: readonly DetectedSource[]): ToolGroup[] {
  const t = m().onboarding
  const sessions = (source: string) => detected.find(item => item.source === source)?.sessions ?? 0
  const pill = (total: number, quota: boolean): PrecisionPill => total <= 0
    ? { text: t.notDetected, tone: 'neutral' }
    : { text: quota ? t.tokenAndQuota : t.tokenExact, tone: 'success' }
  const pair = (cli: number, desktop: number): [ToolVariant, ToolVariant] =>
    [variant(t.variantCli, cli > 0, cli), variant(t.variantDesktop, desktop > 0, desktop)]
  const deepseek = detected.find(item => item.source === 'DeepSeek')
  const deepseekSessions = deepseek?.sessions ?? 0
  const install = deepseek?.install ?? { desktop: false, cli: false }
  const desktopFound = install.desktop || (deepseekSessions > 0 && !install.cli)
  const deepseekGroup: ToolGroup = {
    tool: 'DeepSeek',
    name: 'DeepSeek Harness',
    logo: 'deepseek',
    variants: [
      variant(t.variantCli, install.cli, desktopFound ? 0 : deepseekSessions),
      variant(t.variantDesktop, desktopFound, desktopFound ? deepseekSessions : 0),
    ],
    hint: null,
    pill: pill(deepseekSessions, false),
  }
  return [
    {
      tool: 'Claude', name: 'Claude', logo: 'claude',
      variants: pair(sessions('ClaudeCli'), sessions('ClaudeDesktop')),
      hint: sessions('ClaudeCli') > 0 ? t.claudeCliHint : null,
      pill: pill(sessions('ClaudeCli') + sessions('ClaudeDesktop'), false),
    },
    {
      tool: 'Codex', name: 'Codex', logo: 'codex',
      variants: pair(sessions('CodexCli'), sessions('CodexDesktop')),
      hint: null,
      pill: pill(sessions('CodexCli') + sessions('CodexDesktop'), true),
    },
    deepseekGroup,
  ]
}

export interface PrecisionPill {
  text: string
  tone: 'success' | 'neutral'
}

/** One quota source in step 3. Add a row here (plus its setting) to offer a new one. */
export interface QuotaSourceInfo {
  key: keyof OnboardingSelection
  title: string
  description: string
  /** Shown under the description while the switch is on, e.g. a system prompt the user will see. */
  hint?: string
  /** Shown under the description, e.g. why the source does not apply here. */
  notes: string[]
  /** Behind the "?" icon: what turning it on does and why it is worth it. */
  help: string
}

export function quotaSources(kind: DetectionCase = 'normal'): readonly QuotaSourceInfo[] {
  const t = m().onboarding
  const statuslineNotes = kind === 'claude_desktop_only' ? [t.statuslineNeedsCli] : []
  return [
    { key: 'statusline', title: t.statuslineTitle, description: t.statuslineDesc, notes: statuslineNotes, help: t.statuslineHelp },
    { key: 'claudeOfficial', title: t.claudeTitle, description: t.claudeDesc, hint: t.claudeHint, notes: [], help: t.claudeHelp },
    { key: 'codexOfficial', title: t.codexTitle, description: t.codexDesc, notes: [], help: t.codexHelp },
    { key: 'deepseekOfficial', title: m().deepseek.onboardingTitle, description: m().deepseek.onboardingBody, notes: [], help: t.deepseekHelp },
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
  if (selection.deepseekOfficial && result.deepseek) lines.push({ text: m().onboarding.deepseekOutcome(quotaMessage(result.deepseek)), outcome: result.deepseek })
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

/** A requested settings destination is used only after the guide has saved completion. */
export function onboardingExitTarget(calibrationRequested: boolean, completed: boolean): string | null {
  return calibrationRequested && completed ? '/settings?section=calibration&tool=Claude' : null
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
