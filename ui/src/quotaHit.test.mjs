import assert from 'node:assert/strict'
import test from 'node:test'
import { hitAdvice, hitRows, quotaHits, realReading } from './quotaHit.ts'

const now = Date.parse('2026-10-29T12:00:00Z')
const quota = (used, extra = {}) => ({
  used_percent: used, resets_at: '2026-10-29T14:00:00Z', origin: 'Official', forecast: 'Unknown',
  window_passed: false, as_of: '2026-10-29T11:58:00Z', estimated_percent: null, ...extra,
})
const tool = (name, fiveHour, weekly = quota(20)) => ({ tool: name, plan: null, subscription_usd: null, five_hour: fiveHour, weekly })

test('a reading of 95 triggers; 94.9 does not', () => {
  assert.deepEqual(quotaHits([tool('Claude', quota(95))], 95, now).map(hit => [hit.tool, hit.kind, hit.reading]), [['Claude', 'FiveHour', 95]])
  assert.deepEqual(quotaHits([tool('Claude', quota(94.9))], 95, now), [])
})

test('a reading of 90 plus a 6 point estimate does not trigger', () => {
  const display = quota(96, { estimated_percent: 6 })
  assert.equal(realReading(display, now), 90)
  assert.deepEqual(quotaHits([tool('Claude', display)], 95, now), [])
})

test('the threshold comes from the caller', () => {
  assert.equal(quotaHits([tool('Claude', quota(91))], 90, now).length, 1)
})

test('a passed window does not trigger', () => {
  assert.deepEqual(quotaHits([tool('Claude', quota(97, { window_passed: true }))], 95, now), [])
  assert.deepEqual(quotaHits([tool('Claude', quota(97, { resets_at: '2026-10-29T11:00:00Z' }))], 95, now), [])
})

test('a pure estimate never counts as a reading', () => {
  assert.deepEqual(quotaHits([tool('Claude', quota(97, { origin: 'Estimated' }))], 95, now), [])
  assert.deepEqual(quotaHits([tool('Claude', quota(97, { as_of: null }))], 95, now), [])
  assert.deepEqual(quotaHits([tool('Claude', quota(null))], 95, now), [])
})

test('tools and windows are judged separately', () => {
  const hits = quotaHits([tool('Claude', quota(30), quota(96, { origin: 'Statusline' })), tool('Codex', quota(98, { origin: 'Log' }))], 95, now)
  assert.deepEqual(hits.map(hit => [hit.tool, hit.kind, hit.origin]), [['Claude', 'Weekly', 'Statusline'], ['Codex', 'FiveHour', 'Log']])
})

const rank = (key, delta) => ({ key, name: key, subtitle: '', tokens: 0, requests: 1, delta })
const detail = (sessions, subagents, unattributed) => ({ sessions, subagents, unattributed_total: unattributed })

test('top three sessions, the subagent total, and unattributed only when larger than the third', () => {
  const sessions = [rank('a', 30), rank('b', 20), rank('c', 10), rank('d', 5)]
  const subagents = [rank('main', 50), rank('subagent', 8)]
  assert.deepEqual(hitRows(detail(sessions, subagents, 12)).map(row => [row.kind, row.delta]),
    [['session', 30], ['session', 20], ['session', 10], ['subagent', 8], ['unattributed', 12]])
  assert.deepEqual(hitRows(detail(sessions, [rank('main', 60)], 9)).map(row => row.kind), ['session', 'session', 'session'])
  assert.deepEqual(hitRows(detail([], [], 97)).map(row => [row.kind, row.delta]), [['unattributed', 97]])
})

test('advice comes from the same tool, largest avoidable weekly share first', () => {
  const item = (id, tool, pct, avoidable = true) => ({ id, key: id, tool, avoidable, avoidable_weekly_pct: pct, avoidable_cost_usd: 0 })
  const report = { tools: [
    { tool: 'Claude', items: [item('small', 'Claude', 1), item('big', 'Claude', 4), item('startup', 'Claude', 9, false)] },
    { tool: 'Codex', items: [item('codex', 'Codex', 8)] },
  ] }
  assert.equal(hitAdvice(report, 'Claude')?.id, 'big')
  assert.equal(hitAdvice(report, 'Codex')?.id, 'codex')
  assert.equal(hitAdvice({ tools: [] }, 'Claude'), null)
  assert.equal(hitAdvice(null, 'Claude'), null)
})
