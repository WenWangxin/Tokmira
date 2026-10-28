import assert from 'node:assert/strict'
import test from 'node:test'
import { compactNumber, compactText, locale } from './i18n/index.ts'
import { sourceRows } from './sourceStatus.ts'
import { onboardingSummary, quotaOutcome } from './onboardingLogic.ts'
import { defaultShareOptions, shareMarkdown } from './shareLogic.ts'

test('English quota units, source rows, setup summary and share export', () => {
  locale.value = 'en'
  try {
    assert.deepEqual(compactNumber(12_340), { value: '12.3', unit: 'K' })
    assert.equal(compactText(2_500_000), '2.5M')
    assert.equal(compactText(1_200_000_000), '1.2B')
    const rows = sourceRows({
      claudeOfficialEnabled: true, codexOfficialEnabled: false,
      recorder: { statusline_installed: false, statusline_modified: false, last_statusline_record: null },
      times: null, claudeNeedsAuthorization: false, lastResult: null, now: Date.now(),
    })
    assert.equal(rows[0].title, 'Claude official query')
    assert.equal(rows[0].status, 'No data yet')
    assert.equal(rows[2].status, 'Off')
    const selection = { statusline: false, claudeOfficial: true, codexOfficial: true }
    const outcomes = quotaOutcome(selection, {
      claude: { state: 'error', message: 'Could not connect to Anthropic' },
      codex: { state: 'updated', message: '' },
    })
    assert.deepEqual(onboardingSummary(outcomes), {
      text: 'Claude official quota: Could not connect to Anthropic', tone: 'warning',
    })
    const markdown = shareMarkdown({
      year: 2026, month: 10, month_estimated_cost_usd: 187.4,
      tools: [{ tool: 'Claude', plan: 'pro', subscription_usd: 20, estimated_cost_usd: 187.4 }],
      recognized_subscriptions: 1, subscription_total_usd: 20, subscription_value_multiple: 9.37,
      unrecognized_tools: [], cache_hit_rate: 0.68, requests: 1284, top_project: null,
    }, defaultShareOptions())
    assert.match(markdown, /^# Tokmira · October usage report/m)
    assert.match(markdown, /Requests this month: 1,284/)
    assert.match(markdown, /data never left this device/)
    assert.doesNotMatch(markdown, /[一-龥]/)
  } finally {
    locale.value = 'zh'
  }
})
