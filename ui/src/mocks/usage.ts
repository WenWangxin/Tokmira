import type { Tool, UsageDetail, UsageRank, UsageResponse, UsageWindowRef } from '../api'

// Synthetic browser preview only; no user logs or credentials.
const now = new Date()
const at = (minutes: number) => new Date(now.getTime() + minutes * 60_000).toISOString()
const windowRef = (tool: Tool, kind: UsageWindowRef['kind'], start: number, end: number): UsageWindowRef => ({
  tool, kind, window_id: at(end).slice(0, 16) + ':00Z', start: at(start), end: at(end),
  is_current: start <= 0 && end > 0,
})
const rank = (key: string, name: string, subtitle: string, tokens: number, delta: number, requests = 3): UsageRank =>
  ({ key, name, subtitle, tokens, requests, delta })

export function mockUsageDetail(window: UsageWindowRef): UsageDetail {
  const empty = new URLSearchParams(location.search).get('mock') === 'empty'
  const visibleEnd = Math.min(Date.now(), Date.parse(window.end))
  const range = Math.max(1, visibleEnd - Date.parse(window.start))
  const promptAt = (fraction: number) => new Date(Date.parse(window.start) + range * fraction).toISOString()
  const sessions = empty ? [] : [
    rank('sample-session-01', '重构登录模块', '示例项目 · 命令行', 1240000, 9.6, 12),
    rank('sample-session-02', '写对账单元测试', '示例项目 · 桌面端', 760000, 5.8, 8),
    rank('sample-session-03', '检查缓存逻辑', '审计工具 · 命令行', 510000, 3.9, 5),
    rank('sample-session-04', '修复报告', '审计工具 · 桌面端', 220000, 1.7, 2),
  ]
  const projects = empty ? [] : [rank('/sample/project', '示例项目', '2 个会话', 2000000, 15.4, 20), rank('/sample/audit', '审计工具', '2 个会话', 730000, 5.6, 7)]
  const models = empty ? [] : [rank('sample-model-large', '示例大模型', '19 次请求', 1840000, 14.2, 19), rank('sample-model-small', '示例小模型', '8 次请求', 890000, 6.8, 8)]
  const subagents = empty ? [] : [rank('main', '主对话', '22 次请求', 2400000, 18.2, 22), rank('subagent', '子代理', '5 次请求', 330000, 2.8, 5)]
  const high_prompts = empty ? [] : [
    { at: promptAt(.85), session_id: 'sample-session-01', title: '重构登录模块', delta: 4.2, requests: 3, tokens: 450000 },
    { at: promptAt(.7), session_id: 'sample-session-01', title: '重构登录模块', delta: 4.0, requests: 2, tokens: 390000 },
    { at: promptAt(.5), session_id: 'sample-session-02', title: '写对账单元测试', delta: 3.9, requests: 4, tokens: 350000 },
    { at: promptAt(.35), session_id: 'sample-session-02', title: '写对账单元测试', delta: 3.6, requests: 2, tokens: 310000 },
  ]
  return {
    window, used_percent: 28, remaining_percent: 72, resets_at: window.end,
    origin: window.tool === 'Claude' ? 'Statusline' : 'Log',
    attributed_total: empty ? 0 : 21, unattributed_total: empty ? 28 : 7,
    sessions, projects, models, subagents, high_prompts,
    official_weekly_breakdown: window.tool === 'Claude' && window.kind === 'Weekly' ? {
      as_of: at(-10), window_started_at: window.start, rows: [
        { key: 'claude_code', display_name: 'Claude Code', percent: 98 },
        { key: 'chat', display_name: '聊天', percent: 0 },
        { key: 'cowork', display_name: 'Cowork', percent: 2 },
        { key: 'other', display_name: '其他', percent: 0 },
      ],
    } : null,
  }
}

export function mockUsage(tool: Tool, variant: 'default' | 'empty' | 'nodata'): UsageResponse {
  if (variant === 'nodata') return { windows: [], detail: null }
  const windows = [windowRef(tool, 'FiveHour', -280, 20), windowRef(tool, 'FiveHour', -600, -300),
    windowRef(tool, 'FiveHour', -920, -620), windowRef(tool, 'Weekly', -7500, 2580)]
  return { windows, detail: mockUsageDetail(windows[0]!) }
}
