import assert from 'node:assert/strict'
import test from 'node:test'
import { QUOTA_POLL_MS, nextAuthorizationState, quotaPollDue, refreshDelay } from './realtime.ts'

test('quota poll runs only while visible and at most once a minute', () => {
  assert.equal(quotaPollDue(1_000, null, true), true)
  assert.equal(quotaPollDue(1_000, null, false), false)
  assert.equal(quotaPollDue(QUOTA_POLL_MS - 1, 0, true), false)
  assert.equal(quotaPollDue(QUOTA_POLL_MS, 0, true), true)
  assert.equal(quotaPollDue(QUOTA_POLL_MS * 5, 0, false), false)
})

test('page refreshes are spaced at least a second apart', () => {
  assert.equal(refreshDelay(5_000, null), 0)
  assert.equal(refreshDelay(5_000, 4_500), 500)
  assert.equal(refreshDelay(5_000, 3_000), 0)
  assert.equal(refreshDelay(5_000, 4_900, 200), 100)
})

test('authorization prompt appears on a silent failure and clears after a successful query', () => {
  assert.equal(nextAuthorizationState(false, 'needs_authorization'), true)
  assert.equal(nextAuthorizationState(true, 'recent'), true)
  assert.equal(nextAuthorizationState(true, 'off'), true)
  assert.equal(nextAuthorizationState(true, 'updated'), false)
  assert.equal(nextAuthorizationState(false, 'error'), false)
})
