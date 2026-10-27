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
  assert.equal(nextAuthorizationState(false, '需要授权'), true)
  assert.equal(nextAuthorizationState(true, '60 秒内已查询'), true)
  assert.equal(nextAuthorizationState(true, '未开启'), true)
  assert.equal(nextAuthorizationState(true, '已更新'), false)
  assert.equal(nextAuthorizationState(false, '无法连接 Anthropic'), false)
})
