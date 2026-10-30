import assert from 'node:assert/strict'
import test from 'node:test'
import { findAdviceTarget, readAdviceDeepLink } from './adviceRouteLogic.ts'

const report = { tools: [
  { tool: 'Claude', items: [{ key: 'claude:cache_rebuild' }] },
  { tool: 'Codex', items: [{ key: 'codex:heavy_reasoning' }] },
] }

test('an empty query leaves the saved advice selection alone', () => {
  assert.deepEqual(readAdviceDeepLink({}), { tool: null, key: null })
})

test('reads an exact tool and advice key', () => {
  assert.deepEqual(readAdviceDeepLink({ tool: 'Codex', key: 'codex:heavy_reasoning' }),
    { tool: 'Codex', key: 'codex:heavy_reasoning' })
  assert.equal(findAdviceTarget(report, 'Codex', 'codex:heavy_reasoning'), 'codex:heavy_reasoning')
})

test('ignores malformed query values', () => {
  assert.deepEqual(readAdviceDeepLink({ tool: 'Unknown', key: ['claude:cache_rebuild'] }), { tool: null, key: null })
  assert.deepEqual(readAdviceDeepLink({ tool: 'Unknown', key: 'claude:cache_rebuild' }), { tool: null, key: null })
  assert.deepEqual(readAdviceDeepLink({ tool: ['Claude'], key: '' }), { tool: null, key: null })
})

test('only finds a key in the selected tool after advice data is available', () => {
  assert.equal(findAdviceTarget(null, 'Claude', 'claude:cache_rebuild'), null)
  assert.equal(findAdviceTarget(report, 'Codex', 'claude:cache_rebuild'), null)
  assert.equal(findAdviceTarget(report, 'Claude', 'claude:missing'), null)
})
