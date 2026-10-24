import type { SessionDetail, SessionRow, Source, TimelineEntry, TokenCounts, Tool } from '../api'

// Synthetic browser preview only. No user logs, prompts, or credentials.
const counts = (partial: Partial<TokenCounts>): TokenCounts => ({
  input: 0, output: 0, reasoning: 0, cache_read: 0,
  cache_write_5m: 0, cache_write_1h: 0, cache_write_unspecified: 0, ...partial,
})
const clock = (hours: number, minutes: number, seconds: number) => {
  const date = new Date()
  date.setHours(hours, minutes, seconds, 0)
  return date.toISOString()
}
const day = (offset: number) => {
  const date = new Date()
  date.setDate(date.getDate() + offset)
  date.setHours(15, 0, 0, 0)
  return date.toISOString()
}

function row(
  tool: Tool, sessionId: string, title: string | null, source: Source, project: string, model: string,
  requests: number, total: number, hit: number, weekly: number, cost: number,
): SessionRow {
  const projectName = project.split(/[\\/]/).filter(Boolean).at(-1) ?? ''
  const display = title?.trim() ? title : projectName ? `${projectName} · ${sessionId.slice(0, 8)}` : sessionId.slice(0, 8)
  return {
    tool, session_id: sessionId, title, display_name: display, project, project_name: projectName, source, model,
    requests, total_tokens: total, cache_hit_rate: hit, estimated_cost_usd: cost, weekly_delta: weekly,
    first_at: day(-2), last_at: day(0), has_quota_data: true,
  }
}

export function mockSessions(): SessionRow[] {
  return [
    row('Codex', 'sample-codex-01', '迁移数据库脚本', 'CodexDesktop', '/sample/Energy', 'gpt-6-sol', 29, 1_630_000, 0.64, 11.2, 18.4),
    row('Claude', 'sample-claude-02', '重构登录模块', 'ClaudeCli', '/sample/token-audit', 'claude-opus-5-5', 37, 1_240_000, 0.71, 9.6, 22.1),
    row('Codex', 'sample-codex-03', '写 README', 'CodexCli', '/sample/token-audit', 'gpt-6-sol', 12, 930_000, 0.82, 6.4, 8.2),
    row('Claude', 'sample-claude-04', '写对账单元测试', 'ClaudeDesktop', '/sample/token-audit', 'claude-opus-5-5', 24, 760_000, 0.48, 5.8, 9.5),
    row('Claude', 'sample-claude-05', '看 Sub2API 源码', 'ClaudeCli', '/sample/Sub2Api', 'claude-sonnet-5-5', 19, 510_000, 0.77, 3.9, 4.4),
    row('Codex', 'sample-codex-06', '排查前端白屏', 'CodexDesktop', '/sample/gaobao-front', 'gpt-6-sol', 9, 490_000, 0.59, 3.4, 3.1),
    row('Claude', 'sample-claude-07', '修 CI 报错', 'ClaudeDesktop', '/sample/gaobao-api', 'claude-sonnet-5-5', 8, 220_000, 0.66, 1.7, 1.2),
    row('Codex', 'abcd1234-exec', null, 'CodexCli', '/sample/gaobao-api', 'gpt-6-sol', 3, 80_000, 0.22, 0.4, 0.6),
  ]
}

function featured(base: SessionRow): SessionDetail {
  const first = clock(14, 52, 3)
  const secondPrompt = clock(14, 58, 40)
  const second = clock(14, 58, 44)
  const thirdPrompt = clock(16, 10, 12)
  const third = clock(16, 10, 15)
  const fourth = clock(16, 10, 52)
  const entries: TimelineEntry[] = [
    { kind: 'Prompt', at: first, gap_minutes_from_previous_prompt: null },
    {
      kind: 'Request', at: first, index: 1, is_subagent: false, model: base.model, weekly_delta: 4,
      tokens: counts({ cache_write_1h: 45_315, output: 412 }),
      tag: { tag: '新会话启动开销', severity: 'Warning' },
    },
    { kind: 'Prompt', at: secondPrompt, gap_minutes_from_previous_prompt: 6 },
    {
      kind: 'Request', at: second, index: 2, is_subagent: false, model: base.model, weekly_delta: 0.6,
      tokens: counts({ cache_write_5m: 3_210, cache_read: 45_315, output: 1_806 }), tag: null,
    },
    { kind: 'Prompt', at: thirdPrompt, gap_minutes_from_previous_prompt: 71 },
    {
      kind: 'Request', at: third, index: 3, is_subagent: false, model: base.model, weekly_delta: 3.6,
      tokens: counts({ cache_write_1h: 61_504, output: 956 }),
      tag: { tag: '缓存已过期', severity: 'Error' },
    },
    {
      kind: 'Request', at: fourth, index: 4, is_subagent: true, model: base.model, weekly_delta: 1.1,
      tokens: counts({ cache_write_5m: 22_140, cache_read: 8_770, output: 530 }),
      tag: { tag: '子代理', severity: 'Warning' },
    },
  ]
  return { ...base, entries }
}

function generic(base: SessionRow): SessionDetail {
  const prompt = clock(11, 4, 10)
  const again = clock(11, 46, 10)
  const claude = base.tool === 'Claude'
  const entries: TimelineEntry[] = [
    { kind: 'Prompt', at: prompt, gap_minutes_from_previous_prompt: null },
    {
      kind: 'Request', at: prompt, index: 1, is_subagent: false, model: base.model, weekly_delta: 0.4,
      tokens: claude ? counts({ cache_write_1h: 12_000, output: 180 }) : counts({ input: 9_400, cache_read: 200, output: 180 }),
      tag: { tag: '新会话启动开销', severity: 'Warning' },
    },
    { kind: 'Prompt', at: again, gap_minutes_from_previous_prompt: 42 },
    {
      kind: 'Request', at: again, index: 2, is_subagent: true, model: base.model, weekly_delta: 0,
      tokens: claude ? counts({ cache_read: 12_000, output: 90 }) : counts({ input: 1_200, cache_read: 8_000, output: 90 }),
      tag: null,
    },
  ]
  return { ...base, requests: 2, total_tokens: claude ? 24_270 : 19_070, weekly_delta: 0.4, entries }
}

export function mockSessionDetail(tool: Tool, sessionId: string): SessionDetail | null {
  const found = mockSessions().find(item => item.tool === tool && item.session_id === sessionId)
  if (!found) return null
  return sessionId === 'sample-claude-02' ? featured(found) : generic(found)
}
