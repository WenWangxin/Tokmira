import type { DetectedTool } from '../api'

/**
 * Handwritten browser-preview detection.
 * Both tools have logs, Codex plan is Plus, Claude plan is unknown.
 * `nodesktop` clears the Claude desktop row.
 */
export function mockDetectedTools(variant: string | null): DetectedTool[] {
  const desktop = variant === 'nodesktop'
    ? { sessions: 0, requests: 0, last_used_at: null }
    : { sessions: 6, requests: 18, last_used_at: '2026-10-23T08:00:00.000Z' }
  return [
    { source: 'ClaudeCli', sessions: 34, requests: 210, last_used_at: '2026-10-24T11:20:00.000Z', plan: null },
    { source: 'ClaudeDesktop', plan: null, ...desktop },
    { source: 'CodexCli', sessions: 4, requests: 22, last_used_at: '2026-10-24T09:04:00.000Z', plan: 'plus' },
    { source: 'CodexDesktop', sessions: 15, requests: 40, last_used_at: '2026-10-24T10:15:00.000Z', plan: 'plus' },
  ]
}
