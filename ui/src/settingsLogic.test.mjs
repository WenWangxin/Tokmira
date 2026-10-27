import assert from 'node:assert/strict'
import test from 'node:test'
import { reactive } from 'vue'
import { adviceLabel, clonePreview, formatCalibration, parsePercent, themeDataset } from './settingsLogic.ts'

test('system theme clears the document attribute', () => {
  assert.equal(themeDataset('system'), null)
  assert.equal(themeDataset('light'), 'light')
  assert.equal(themeDataset('dark'), 'dark')
})

test('dismissed advice keys use the rule title', () => {
  assert.equal(adviceLabel('claude:cache_rebuild'), 'Claude · 缓存过期后的整段重建')
  assert.equal(adviceLabel('codex:many_subagents'), 'Codex · 子代理使用过多')
  assert.equal(adviceLabel('claude:expensive_light_task'), 'Claude · 轻量任务使用了高价模型')
  assert.equal(adviceLabel('codex:runaway_prompt'), 'Codex · 一次提示跑了太多轮')
})

test('preview copy accepts a Vue reactive settings object', () => {
  const settings = reactive({
    onboarding_completed: true,
    claude_official_query: false,
    claude_credential: 'Auto',
    codex_app_server: true,
    theme: 'dark',
    calibration: {
      tool: 'Claude',
      five_hour_percent: 28,
      recorded_at: '2026-10-24T06:30:00.000Z',
    },
    dismissed_advice: ['claude:long_context'],
  })
  const copy = clonePreview(settings)
  assert.deepEqual(copy, {
    onboarding_completed: true,
    claude_official_query: false,
    claude_credential: 'Auto',
    codex_app_server: true,
    theme: 'dark',
    calibration: {
      tool: 'Claude',
      five_hour_percent: 28,
      recorded_at: '2026-10-24T06:30:00.000Z',
    },
    dismissed_advice: ['claude:long_context'],
  })
  copy.theme = 'light'
  copy.calibration.five_hour_percent = 0
  copy.dismissed_advice.push('codex:many_subagents')
  assert.equal(settings.theme, 'dark')
  assert.equal(settings.calibration.five_hour_percent, 28)
  assert.deepEqual(settings.dismissed_advice, ['claude:long_context'])
})

test('calibration percent and local timestamp', () => {
  assert.equal(parsePercent(''), null)
  assert.equal(parsePercent('28'), 28)
  assert.equal(parsePercent('100.5'), null)
  assert.equal(parsePercent('-1'), null)
  assert.equal(formatCalibration('Claude', 28, new Date(2026, 9, 24, 14, 30)), '上次校准：Claude 28% · 10/24 14:30')
})
