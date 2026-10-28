import assert from 'node:assert/strict'
import test from 'node:test'
import {
  costBarSegments,
  defaultShareOptions,
  exportFileName,
  formatPercent,
  formatSubscriptionUsd,
  formatUsd,
  projectLine,
  shareMarkdown,
  shareTitle,
  subscriptionCaption,
  svgDataUri,
  toolRowLabel,
  unrecognizedNote,
  valuePillText,
} from './shareLogic.ts'

const tool = (name, plan, price, cost) => ({
  tool: name,
  plan,
  subscription_usd: price,
  estimated_cost_usd: cost,
})

function summary(patch = {}) {
  return {
    year: 2026,
    month: 10,
    month_estimated_cost_usd: 187.4,
    tools: [tool('Claude', 'pro', 20, 121.8), tool('Codex', 'plus', 20, 65.6)],
    recognized_subscriptions: 2,
    subscription_total_usd: 40,
    subscription_value_multiple: 4.685,
    unrecognized_tools: [],
    cache_hit_rate: 0.68,
    requests: 1284,
    top_project: { name: 'token-audit', estimated_cost_usd: 65.6 },
    ...patch,
  }
}

test('subscription caption follows how many plans were recognized', () => {
  assert.equal(subscriptionCaption(summary()), '两份订阅共 $40，折合 API 费用')
  assert.equal(subscriptionCaption(summary({
    recognized_subscriptions: 1,
    subscription_total_usd: 20,
  })), '订阅 $20，折合 API 费用')
  assert.equal(subscriptionCaption(summary({
    recognized_subscriptions: 0,
    subscription_total_usd: null,
    subscription_value_multiple: null,
  })), '本月折合 API 费用')
  assert.equal(formatSubscriptionUsd(20.5), '$20.50')
  assert.equal(formatUsd(187.4), '$187.40')
  assert.equal(formatUsd(1284.5), '$1,284.50')
})

test('value pill and unrecognized note stay in step', () => {
  assert.equal(valuePillText(4.685), '价值约为订阅的 4.7 倍')
  assert.equal(valuePillText(null), null)
  assert.equal(unrecognizedNote(summary()), null)
  assert.equal(unrecognizedNote(summary({
    unrecognized_tools: ['Claude'],
  })), '不含 Claude（套餐未识别）')
  assert.equal(unrecognizedNote(summary({
    subscription_value_multiple: null,
    unrecognized_tools: ['Claude', 'Codex'],
  })), null)
  assert.equal(unrecognizedNote(summary({
    unrecognized_tools: ['Claude', 'Codex'],
  })), '不含 Claude、Codex（套餐未识别）')
})

test('tool labels use the plan only when it has a price', () => {
  assert.equal(toolRowLabel(tool('Claude', 'pro', 20, 1)), 'Claude Pro')
  assert.equal(toolRowLabel(tool('Claude', 'max-20x', 200, 1)), 'Claude Max 20x')
  assert.equal(toolRowLabel(tool('Codex', 'Plus', 20, 1)), 'ChatGPT Plus · Codex')
  assert.equal(toolRowLabel(tool('Codex', 'pro', 200, 1)), 'ChatGPT Pro · Codex')
  assert.equal(toolRowLabel(tool('Claude', 'team', null, 1)), 'Claude')
  assert.equal(toolRowLabel(tool('Codex', null, null, 1)), 'Codex')
  assert.equal(toolRowLabel(tool('Claude', 'pro', null, 1)), 'Claude')
})

test('markdown follows the toggles and never invents a path', () => {
  const data = summary()
  const markdown = shareMarkdown(data, defaultShareOptions())
  assert.match(markdown, /^# Tokmira · 10 月用量报告/)
  assert.match(markdown, /两份订阅共 \$40，折合 API 费用：\$187\.40/)
  assert.match(markdown, /Claude Pro：\$121\.80/)
  assert.match(markdown, /ChatGPT Plus · Codex：\$65\.60/)
  assert.match(markdown, /价值约为订阅的 4\.7 倍/)
  assert.match(markdown, /缓存命中率：68%/)
  assert.match(markdown, /本月请求：1,284 次/)
  assert.equal(markdown.includes('消耗最高的项目'), false)
  assert.equal(markdown.includes('/'), false)
  assert.match(markdown, /金额为 API 等价估算，并非实际账单；基于本机日志核算，数据未离开设备。$/)

  const hidden = shareMarkdown(data, { toolDetails: false, cacheHitRate: false, topProject: true })
  assert.equal(hidden.includes('Claude Pro'), false)
  assert.equal(hidden.includes('缓存命中率'), false)
  assert.match(hidden, /消耗最高的项目：token-audit · \$65\.60/)
  assert.match(hidden, /本月请求：1,284 次/)

  const emptyProject = shareMarkdown(summary({ top_project: null }), {
    toolDetails: false,
    cacheHitRate: false,
    topProject: true,
  })
  assert.match(emptyProject, /消耗最高的项目：暂无/)
  assert.equal(shareTitle(data), 'Tokmira · 10 月用量报告')
  assert.equal(formatPercent(null), '—')
  assert.equal(projectLine({ name: '这是一个很长的项目目录名称用于截断', estimated_cost_usd: 1 }, 8), '消耗最高的项目：这是一个很长的… · $1.00')
})

test('bar segments use cost share and export names are dated', () => {
  const segments = costBarSegments(summary().tools, 616)
  assert.deepEqual(segments.map(item => item.tool), ['Claude', 'Codex'])
  assert.equal(segments.reduce((sum, item) => sum + item.width, 0), 616)
  assert.equal(segments[0].width, 400)
  assert.equal(segments[1].width, 216)
  assert.deepEqual(costBarSegments([tool('Codex', 'plus', 20, 3)], 616), [{ tool: 'Codex', width: 616 }])
  assert.deepEqual(costBarSegments([tool('Claude', 'pro', 20, 0)], 616), [])
  assert.equal(exportFileName(summary(), 'png'), 'tokmira-2026-10.png')
  assert.equal(exportFileName(summary({ month: 1 }), 'md'), 'tokmira-2026-01.md')
  const uri = svgDataUri('<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>')
  assert.equal(uri.startsWith('data:image/svg+xml'), true)
  assert.equal(uri.includes('http://') || uri.includes('https://'), false)
})
