import test from 'node:test'
import assert from 'node:assert/strict'
import { pageCacheKey, keysToInvalidate, getPageCache, setPageCache, cacheVersion, cacheVersionMatches, invalidatePageCache } from './pageCache.ts'

test('keys distinguish page parameters and encode window ids', () => {
  assert.notEqual(pageCacheKey('advice', 7), pageCacheKey('advice', 30))
  assert.notEqual(pageCacheKey('usageDetail', 'Claude', 'Weekly', 'a:b'), pageCacheKey('usageDetail', 'Claude', 'Weekly', 'a', 'b'))
  assert.notEqual(pageCacheKey('sessions', 7), pageCacheKey('sessions', 30))
})

test('invalidation selects only the specified groups and rejects stale responses', () => {
  const advice = pageCacheKey('advice', 7)
  const overview = pageCacheKey('overview')
  const usage = pageCacheKey('usageDetail', 'Codex', 'FiveHour', 'x')
  assert.deepEqual(keysToInvalidate([advice, overview, usage], ['advice', 'usageDetail']), [advice, usage])
  setPageCache(advice, { count: 1 })
  setPageCache(overview, { count: 2 })
  const version = cacheVersion(advice)
  invalidatePageCache('advice')
  assert.equal(getPageCache(advice), undefined)
  assert.deepEqual(getPageCache(overview), { count: 2 })
  assert.equal(cacheVersionMatches(advice, version), false)
})
