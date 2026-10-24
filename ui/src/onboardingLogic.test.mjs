import assert from 'node:assert/strict'
import test from 'node:test'
import { mockDetectedTools } from './mocks/onboarding.ts'
import {
  claudeQuotaHint,
  initialOnboardingChoices,
  installOutcome,
  onboardingChangeLines,
  outcomeTone,
  precisionPill,
  previewOnboardingForced,
  quotaOutcome,
  toolSubtitle,
} from './onboardingLogic.ts'

test('change list follows the checked sources', () => {
  const none = { statusline: false, claudeOfficial: false, codexOfficial: false }
  assert.deepEqual(onboardingChangeLines(none), ['不会修改任何文件，也不会联网。'])
  assert.deepEqual(onboardingChangeLines({ ...none, statusline: true }), [
    '~/.claude/settings.json → statusLine',
    '安装前自动完整备份，保留原有状态栏，可随时在设置中一键卸载。',
  ])
  assert.deepEqual(onboardingChangeLines({ statusline: true, claudeOfficial: true, codexOfficial: true }), [
    '~/.claude/settings.json → statusLine',
    '安装前自动完整备份，保留原有状态栏，可随时在设置中一键卸载。',
    '将向 api.anthropic.com 查询额度',
    '将启动 codex app-server，由 Codex 向 OpenAI 查询',
  ])
  assert.deepEqual(onboardingChangeLines({ ...none, codexOfficial: true }), [
    '将启动 codex app-server，由 Codex 向 OpenAI 查询',
  ])
})

test('tool subtitle omits an unknown plan and marks a missing log', () => {
  assert.equal(toolSubtitle('ClaudeCli', 34, null), '34 个会话')
  assert.equal(toolSubtitle('ClaudeCli', 34, '  '), '34 个会话')
  assert.equal(toolSubtitle('ClaudeCli', 2, 'pro'), 'Claude Pro · 2 个会话')
  assert.equal(toolSubtitle('CodexCli', 4, 'plus'), 'ChatGPT Plus · 4 个会话')
  assert.equal(toolSubtitle('CodexDesktop', 15, 'Plus'), 'ChatGPT Plus · 15 个会话')
  assert.equal(toolSubtitle('ClaudeDesktop', 0, 'pro'), '未检测到本机日志')
  assert.equal(toolSubtitle('CodexCli', 1, 'enterprise'), 'enterprise · 1 个会话')
})

test('precision pills and the Claude command-line hint', () => {
  assert.deepEqual(precisionPill('ClaudeCli', 0), { text: '未检测到', tone: 'neutral' })
  assert.deepEqual(precisionPill('ClaudeDesktop', 3), { text: 'token 精确', tone: 'success' })
  assert.deepEqual(precisionPill('ClaudeCli', 34), { text: 'token 精确', tone: 'success' })
  assert.deepEqual(precisionPill('CodexCli', 4), { text: 'token 与额度精确', tone: 'success' })
  assert.equal(claudeQuotaHint('ClaudeCli', 34), '额度需开启记录器或官方查询')
  assert.equal(claudeQuotaHint('ClaudeCli', 0), null)
  assert.equal(claudeQuotaHint('ClaudeDesktop', 8), null)
})

test('first run defaults; reopening keeps current switches', () => {
  const enabled = { claude_official_query: true, codex_app_server: true }
  assert.deepEqual(initialOnboardingChoices(false, enabled, false), {
    statusline: true,
    claudeOfficial: false,
    codexOfficial: false,
  })
  assert.deepEqual(initialOnboardingChoices(true, enabled, false), {
    statusline: false,
    claudeOfficial: true,
    codexOfficial: true,
  })
})

test('install and quota sentences, and when the dashboard notice should warn', () => {
  assert.deepEqual(installOutcome('/tmp/backup.json', []), ['已备份到 /tmp/backup.json'])
  assert.deepEqual(installOutcome(null, ['保留原命令']), ['保留原命令'])
  assert.deepEqual(installOutcome(null, []), ['没有修改配置文件'])
  assert.deepEqual(quotaOutcome(
    { statusline: false, claudeOfficial: true, codexOfficial: false },
    { claude: '已更新', codex: '未开启' },
  ), ['Claude 官方额度：已更新'])
  assert.equal(outcomeTone(['已备份到 /tmp/backup.json', 'Claude 官方额度：已更新']), 'success')
  assert.equal(outcomeTone(['Codex 官方额度：未开启']), 'success')
  assert.equal(outcomeTone(['Claude 官方额度：Anthropic 查询已限流']), 'warning')
})

test('preview query forces the guide only in the browser preview', () => {
  assert.equal(previewOnboardingForced('?onboarding=1', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=nodesktop', true), true)
  assert.equal(previewOnboardingForced('', true), false)
  assert.equal(previewOnboardingForced('?onboarding=1', false), false)
})

test('preview fixtures cover both tools and a missing Claude desktop log', () => {
  const detected = mockDetectedTools(null)
  assert.equal(detected.length, 4)
  assert.ok(detected.filter(tool => tool.source.startsWith('Claude')).some(tool => tool.sessions > 0))
  assert.ok(detected.filter(tool => tool.source.startsWith('Codex')).every(tool => tool.sessions > 0 && tool.plan === 'plus'))
  assert.equal(detected.find(tool => tool.source.startsWith('Claude'))?.plan ?? null, null)
  const desktop = mockDetectedTools('nodesktop').find(tool => tool.source === 'ClaudeDesktop')
  assert.ok(desktop)
  assert.equal(desktop.sessions, 0)
  assert.equal(desktop.requests, 0)
  assert.equal(desktop.last_used_at, null)
  assert.equal(toolSubtitle(desktop.source, desktop.sessions, desktop.plan), '未检测到本机日志')
  assert.equal(precisionPill(desktop.source, desktop.sessions).text, '未检测到')
})
