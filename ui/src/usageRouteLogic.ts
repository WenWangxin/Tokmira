import type { Tool, UsageWindowRef } from './api.ts'

export type UsageLink = { tool?: unknown; window?: unknown }

export function readUsageDeepLink(query: Record<string, unknown>): {
  present: boolean; selection: UsageLink | null; help: boolean
} {
  const hasSelection = 'tool' in query || 'window' in query
  return {
    present: hasSelection || 'help' in query,
    selection: hasSelection ? { tool: query.tool, window: query.window } : null,
    help: query.help === 'unattributed',
  }
}

/** A pending deep link wins once; later calls with null follow the saved selection. */
export function resolveUsageSelection(
  link: UsageLink | null,
  currentTool: Tool,
  savedWindowIds: Partial<Record<Tool, string>>,
  windows: Record<Tool, UsageWindowRef[]>,
): { tool: Tool; window: UsageWindowRef | null } {
  const requestedTool = link?.tool === 'Claude' || link?.tool === 'Codex' ? link.tool : null
  const preferredTool = requestedTool ?? currentTool
  const otherTool = preferredTool === 'Claude' ? 'Codex' : 'Claude'
  const tool = windows[preferredTool].length ? preferredTool : windows[otherTool].length ? otherTool : preferredTool
  const available = windows[tool]
  const requestedWindowId = requestedTool && requestedTool !== tool ? null
    : typeof link?.window === 'string' ? link.window : null
  const window = available.find(item => item.window_id === requestedWindowId)
    ?? available.find(item => item.window_id === savedWindowIds[tool])
    ?? available[0] ?? null
  return { tool, window }
}
