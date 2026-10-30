import assert from 'node:assert/strict'
import test from 'node:test'
import { readUsageDeepLink, resolveUsageSelection } from './usageRouteLogic.ts'

const claudeCurrent = { tool: 'Claude', kind: 'FiveHour', window_id: 'claude-current' }
const claudePrevious = { tool: 'Claude', kind: 'FiveHour', window_id: 'claude-previous' }
const codexCurrent = { tool: 'Codex', kind: 'FiveHour', window_id: 'codex-current' }
const codexPrevious = { tool: 'Codex', kind: 'Weekly', window_id: 'codex-previous' }
const windows = { Claude: [claudeCurrent, claudePrevious], Codex: [codexCurrent, codexPrevious] }
const saved = { Claude: 'claude-previous', Codex: 'codex-previous' }

test('no query and help only keep the saved tool and window', () => {
  assert.deepEqual(readUsageDeepLink({}), { present: false, selection: null, help: false })
  assert.deepEqual(readUsageDeepLink({ help: 'unattributed' }), { present: true, selection: null, help: true })
  assert.deepEqual(resolveUsageSelection(null, 'Claude', saved, windows), { tool: 'Claude', window: claudePrevious })
})

test('deep link tool and window override saved selection', () => {
  const link = readUsageDeepLink({ help: 'unattributed', tool: 'Codex', window: 'codex-current' })
  assert.deepEqual(link, { present: true, selection: { tool: 'Codex', window: 'codex-current' }, help: true })
  assert.deepEqual(resolveUsageSelection(link.selection, 'Claude', saved, windows),
    { tool: 'Codex', window: codexCurrent })
})

test('invalid tool does not replace the current tool', () => {
  assert.deepEqual(resolveUsageSelection({ tool: 'Unknown', window: 'codex-current' }, 'Claude', saved, windows),
    { tool: 'Claude', window: claudePrevious })
})

test('refresh without a pending link keeps a manually selected tool and window', () => {
  const afterManualSelection = { Claude: 'claude-previous', Codex: 'codex-current' }
  assert.deepEqual(resolveUsageSelection(null, 'Codex', afterManualSelection, windows),
    { tool: 'Codex', window: codexCurrent })
})

test('missing window falls back to the saved window, then the first available window', () => {
  assert.deepEqual(resolveUsageSelection({ tool: 'Codex', window: 'missing' }, 'Claude', saved, windows),
    { tool: 'Codex', window: codexPrevious })
  assert.deepEqual(resolveUsageSelection({ tool: 'Codex', window: 'missing' }, 'Claude', {}, windows),
    { tool: 'Codex', window: codexCurrent })
})

test('tool without windows falls back to the other tool', () => {
  assert.deepEqual(resolveUsageSelection({ tool: 'Codex', window: 'missing' }, 'Claude', saved, { Claude: windows.Claude, Codex: [] }),
    { tool: 'Claude', window: claudePrevious })
  assert.deepEqual(resolveUsageSelection({ tool: 'Codex', window: 'claude-current' }, 'Claude', saved, { Claude: windows.Claude, Codex: [] }),
    { tool: 'Claude', window: claudePrevious })
})
