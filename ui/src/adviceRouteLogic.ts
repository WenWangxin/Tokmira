import type { AdviceReport, Tool } from './api.ts'

export function readAdviceDeepLink(query: Record<string, unknown>): { tool: Tool | null; key: string | null } {
  const tool = query.tool === 'Claude' || query.tool === 'Codex' ? query.tool : null
  return {
    tool,
    key: (!('tool' in query) || tool) && typeof query.key === 'string' && query.key.length > 0 ? query.key : null,
  }
}

export function findAdviceTarget(report: AdviceReport | null, tool: Tool, key: string | null): string | null {
  if (!key) return null
  return report?.tools.find(entry => entry.tool === tool)?.items.some(item => item.key === key) ? key : null
}
