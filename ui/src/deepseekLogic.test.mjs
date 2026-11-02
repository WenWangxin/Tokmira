import assert from 'node:assert/strict'
import test from 'node:test'
import { costSegments, daysLeftText, daysTone, segmentWidths, yuan } from './deepseekLogic.ts'
import { locale } from './i18n/index.ts'

const overview = (total, days, low = false) => ({
  today_cost: 3.2, today_requests: 4, daily_average: 2.85, days_left: days, low_balance: low, unpriced_requests: 0,
  balance: total === null ? null : { total, topped_up: total, granted: 0, origin: 'Official', as_of: '2026-11-02T00:00:00Z', spent_since: null },
})

test('yuan keeps small spend visible', () => {
  assert.equal(yuan(3.2), '¥3.20')
  assert.equal(yuan(0.0094), '¥0.01')
  assert.equal(yuan(0.004), '<¥0.01')
  assert.equal(yuan(0), '¥0.00')
  assert.equal(yuan(-1.5), '-¥1.50')
  assert.equal(yuan(null), '—')
})

test('days left and its tone', () => {
  assert.equal(daysLeftText(overview(86.4, 30.6)), '约 31 天')
  assert.equal(daysTone(overview(86.4, 30.3)), 'success-text')
  assert.equal(daysLeftText(overview(5.6, 1.9, true)), '约 2 天')
  assert.equal(daysTone(overview(5.6, 1.9, true)), 'warning-text')
  assert.equal(daysLeftText(overview(0.1, 0.2, true)), '不到 1 天')
  assert.equal(daysTone(overview(-0.2, 0, true)), 'error-text')
  assert.equal(daysLeftText(overview(null, null)), '—')
  assert.equal(daysLeftText(overview(10, null)), '近 7 天没有花费')
  locale.value = 'en'
  try {
    assert.equal(daysLeftText(overview(86.4, 1.2)), 'About 1 day')
  } finally {
    locale.value = 'zh'
  }
})

test('cost segments group the tail and list titles last', () => {
  const rank = (key, cost) => ({ key, name: key, project_name: '', tokens: 1, requests: 1, cost })
  const usage = { sessions: [rank('a', 1.42), rank('b', 0.86), rank('c', 0.51), rank('d', 0.29), rank('e', 0)], title_cost: 0.12 }
  const segments = costSegments(usage)
  assert.deepEqual(segments.map(item => [item.key, item.kind]), [['a', 'session'], ['b', 'session'], ['c', 'session'], ['other', 'other'], ['title', 'title']])
  assert.equal(segments[3].cost, 0.29)
  const widths = segmentWidths(segments)
  assert.ok(Math.abs(widths.reduce((sum, value) => sum + value, 0) - 100) < 1e-9)
  assert.ok(Math.abs(widths[0] - 1.42 / 3.2 * 100) < 1e-9)
  assert.deepEqual(segmentWidths([]), [])
})
