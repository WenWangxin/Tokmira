import type { DetectedTool } from '../api'

const NO_LOGS = { sessions: 0, requests: 0, last_used_at: null }
const DEEPSEEK_NONE: DetectedTool = { source: 'DeepSeek', plan: null, ...NO_LOGS, install: { desktop: false, cli: false } }
const DEEPSEEK: DetectedTool = {
  source: 'DeepSeek', sessions: 1, requests: 1, last_used_at: '2026-11-01T08:00:00.000Z', plan: null,
  install: { desktop: true, cli: false },
}

/**
 * Handwritten browser-preview detection.
 * Both tools have logs, Codex plan is Plus, Claude plan is unknown.
 * `nodesktop` clears the Claude desktop row, `nologs` clears every row,
 * `desktoponly` keeps only the Claude desktop row.
 */
export function mockDetectedTools(variant: string | null): DetectedTool[] {
  if (variant === 'nologs') {
    return [...(['ClaudeCli', 'ClaudeDesktop', 'CodexCli', 'CodexDesktop'] as const).map(source => ({ source, plan: null, ...NO_LOGS })), DEEPSEEK_NONE]
  }
  if (variant === 'desktoponly') {
    return [
      { source: 'ClaudeCli', plan: null, ...NO_LOGS },
      { source: 'ClaudeDesktop', sessions: 6, requests: 18, last_used_at: '2026-10-23T08:00:00.000Z', plan: null },
      { source: 'CodexCli', plan: null, ...NO_LOGS },
      { source: 'CodexDesktop', plan: null, ...NO_LOGS },
      DEEPSEEK_NONE,
    ]
  }
  const desktop = variant === 'nodesktop'
    ? NO_LOGS
    : { sessions: 6, requests: 18, last_used_at: '2026-10-23T08:00:00.000Z' }
  return [
    { source: 'ClaudeCli', sessions: 34, requests: 210, last_used_at: '2026-10-24T11:20:00.000Z', plan: null },
    { source: 'ClaudeDesktop', plan: null, ...desktop },
    { source: 'CodexCli', sessions: 4, requests: 22, last_used_at: '2026-10-24T09:04:00.000Z', plan: 'plus' },
    { source: 'CodexDesktop', sessions: 15, requests: 40, last_used_at: '2026-10-24T10:15:00.000Z', plan: 'plus' },
    DEEPSEEK,
  ]
}
