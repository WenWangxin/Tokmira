import assert from 'node:assert/strict'
import test from 'node:test'
import { compactNumber, compactText, locale } from './i18n/index.ts'
import zh from './i18n/zh.ts'
import en from './i18n/en.ts'
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

test('unattributed help says denser readings may make it larger', () => {
  const texts = help => Object.values(help).map(value => typeof value === 'function' ? value('1%', '2%', '3%') : value)
  for (const key of ['denserClaude', 'denserCodex']) {
    assert.match(zh.usage.unattributedHelp[key], /可能因此变大，这是正常的/)
    assert.match(en.usage.unattributedHelp[key], /may get larger\. This is normal/)
  }
  assert.equal(zh.usage.unattributedHelp.accuracyTitle, '怎样更准、怎样变小')
  for (const text of texts(en.usage.unattributedHelp)) assert.doesNotMatch(text, /[一-龥]/)
  assert.deepEqual(Object.keys(en.usage.unattributedHelp), Object.keys(zh.usage.unattributedHelp))
})

test('advice next step copy texts and preview commands', async () => {
  const { mockAdvice } = await import('./mocks/advice.ts')
  assert.equal(zh.advice.copyFailed, '复制失败，请手动选择命令')
  for (const text of [en.advice.nextStep, en.advice.copyCommand, en.advice.copied('/compact'), en.advice.copyFailed]) {
    assert.doesNotMatch(text, /[一-龥]/)
  }
  for (const lang of ['zh', 'en']) {
    locale.value = lang
    try {
      const items = mockAdvice(7, null).tools.flatMap(tool => tool.items)
      for (const item of items) assert.ok(item.next_step?.text, `${item.key} has a next step`)
      // Codex commands are unverified, and reference rules never get a copy button.
      for (const item of items.filter(item => item.tool === 'Codex' || ['session_startup', 'quota_rebalance', 'plan_fit'].includes(item.id))) {
        assert.equal(item.next_step.command, null, item.key)
      }
      if (lang === 'en') for (const item of items) assert.doesNotMatch(item.next_step.text, /[一-龥]/)
    } finally {
      locale.value = 'zh'
    }
  }
})

test('quota hit card names the tool, window, reading source and time', async () => {
  assert.deepEqual(Object.keys(en.dashboard.hit), Object.keys(zh.dashboard.hit))
  assert.equal(zh.dashboard.hit.title('Claude', zh.dashboard.hit.fiveHour, '97', '官方数据 · 官方查询', '14:02'),
    'Claude 5 小时额度已用 97%（官方数据 · 官方查询 · 14:02 读数）')
  const title = en.dashboard.hit.title('Codex', en.dashboard.hit.weekly, '96', en.origin.log, '10/29 09:15')
  assert.equal(title, 'Codex weekly quota 96% used (Official · Logs · read at 10/29 09:15)')
  for (const text of Object.values(en.dashboard.hit).map(value => typeof value === 'function' ? value('Claude', '5-hour', '97', 'Official', '14:02') : value)) assert.doesNotMatch(text, /[一-龥]/)
  const { mockOverview } = await import('./mocks/overview.ts')
  const { quotaHits } = await import('./quotaHit.ts')
  const hit = mockOverview('hit')
  assert.deepEqual(quotaHits(hit.tools, hit.quota_hit_threshold_pct).map(item => [item.tool, item.kind, item.reading]), [['Claude', 'FiveHour', 97]])
  const estimated = mockOverview('hit-estimated')
  assert.deepEqual(quotaHits(estimated.tools, estimated.quota_hit_threshold_pct), [])
})
