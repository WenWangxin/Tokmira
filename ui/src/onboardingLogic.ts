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
  if (!onboardingCompleted) return { statusline: true, claudeOfficial: false, codexOfficial: false }
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
  const known = source.startsWith('Claude') ? CLAUDE_PLANS : CODEX_PLANS
  return known[key] ?? trimmed
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

export function precisionPill(source: string, sessions: number): PrecisionPill {
  if (sessions <= 0) return { text: '未检测到', tone: 'neutral' }
  if (source === 'CodexCli' || source === 'CodexDesktop') return { text: 'token 与额度精确', tone: 'success' }
  return { text: 'token 精确', tone: 'success' }
}

/** Extra line under Claude Code command-line rows that have logs. */
export function claudeQuotaHint(source: string, sessions: number): string | null {
  if (source !== 'ClaudeCli' || sessions <= 0) return null
  return '额度需开启记录器或官方查询'
}

/** Lines inside “将修改以下配置”, in toggle order. */
export function onboardingChangeLines(selection: OnboardingSelection): string[] {
  const lines: string[] = []
  if (selection.statusline) {
    lines.push('~/.claude/settings.json → statusLine')
    lines.push('安装前自动完整备份，保留原有状态栏，可随时在设置中一键卸载。')
  }
  if (selection.claudeOfficial) lines.push('将向 api.anthropic.com 查询额度')
  if (selection.codexOfficial) lines.push('将启动 codex app-server，由 Codex 向 OpenAI 查询')
  if (!lines.length) lines.push('不会修改任何文件，也不会联网。')
  return lines
}

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

/** Browser preview only. Packaged and Tauri sessions follow the saved setting. */
export function previewOnboardingForced(search: string, isPreview: boolean): boolean {
  if (!isPreview) return false
  return new URLSearchParams(search).get('onboarding') === '1'
}
