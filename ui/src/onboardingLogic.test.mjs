import assert from 'node:assert/strict'
import test from 'node:test'
import { mockDetectedTools } from './mocks/onboarding.ts'
import { m } from './i18n/index.ts'
import {
  detectionCase,
  quotaSources,
  notificationLine,
  trayPlace,
  toolSources,
  detectedToolRows,
  initialOnboardingChoices,
  installOutcome,
  onboardingSummary,
  outcomeTone,
  precisionPill,
  previewOnboardingForced,
  quotaOutcome,
  toolSubtitle,
} from './onboardingLogic.ts'

test('quota sources come from the registry, one per switch', () => {
  assert.deepEqual(quotaSources().map(item => item.key), ['statusline', 'claudeOfficial', 'codexOfficial'])
  assert.equal(quotaSources()[0].title, 'Claude Code 状态栏记录')
})

test('tool rows follow the registry and fall back for unknown sources', () => {
  const rows = detectedToolRows([
    { source: 'ClaudeCli', sessions: 34, requests: 900, plan: null },
    { source: 'CodexCli', sessions: 4, requests: 60, plan: 'plus' },
    { source: 'GeminiCli', sessions: 2, requests: 10, plan: null },
    { source: 'Unknown', sessions: 1, requests: 1, plan: null },
  ])
  assert.deepEqual(rows.map(row => row.source), [...toolSources().map(info => info.source), 'GeminiCli'])
  const claude = rows[0]
  assert.equal(claude.subtitle, '34 个会话')
  assert.equal(claude.hint, '额度需开启记录器或官方查询')
  assert.deepEqual(claude.pill, { text: 'token 精确', tone: 'success' })
  const desktop = rows.find(row => row.source === 'ClaudeDesktop')
  assert.equal(desktop.subtitle, '未检测到本机日志')
  assert.equal(desktop.hint, null)
  assert.deepEqual(rows.find(row => row.source === 'CodexCli').pill, { text: 'token 与额度精确', tone: 'success' })
  const unknown = rows.at(-1)
  assert.equal(unknown.name, 'GeminiCli')
  assert.equal(unknown.logo, 'generic')
  assert.equal(unknown.subtitle, '2 个会话')
  assert.deepEqual(unknown.pill, { text: 'token 精确', tone: 'success' })
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

test('precision pills', () => {
  assert.deepEqual(precisionPill('ClaudeCli', 0), { text: '未检测到', tone: 'neutral' })
  assert.deepEqual(precisionPill('ClaudeDesktop', 3), { text: 'token 精确', tone: 'success' })
  assert.deepEqual(precisionPill('ClaudeCli', 34), { text: 'token 精确', tone: 'success' })
  assert.deepEqual(precisionPill('CodexCli', 4), { text: 'token 与额度精确', tone: 'success' })
})

test('first run defaults; reopening keeps current switches', () => {
  const enabled = { claude_official_query: true, codex_app_server: true }
  const disabled = { claude_official_query: false, codex_app_server: false }
  // First run: status line on, both official queries off, whatever the stored settings say.
  assert.deepEqual(initialOnboardingChoices(false, disabled, false), {
    statusline: true,
    claudeOfficial: false,
    codexOfficial: false,
  })
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
  assert.deepEqual(installOutcome(null, ['statusline_modified']), ['statusLine 已被用户修改，保留当前配置'])
  assert.deepEqual(installOutcome(null, []), ['没有修改配置文件'])
  const result = { claude: { state: 'updated', message: '' }, codex: { state: 'off', message: '' } }
  assert.deepEqual(quotaOutcome(
    { statusline: false, claudeOfficial: true, codexOfficial: false }, result,
  ), [{ text: 'Claude 官方额度：已更新', outcome: result.claude }])
  assert.equal(outcomeTone(quotaOutcome({ statusline: false, claudeOfficial: true, codexOfficial: true }, result)), 'success')
  assert.equal(outcomeTone(quotaOutcome({ statusline: false, claudeOfficial: true, codexOfficial: false }, { ...result, claude: { state: 'error', message: '无法连接 Anthropic' } })), 'warning')
})

test('preview query forces the guide only in the browser preview', () => {
  assert.equal(previewOnboardingForced('?onboarding=1', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=nodesktop', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=nologs', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=desktoponly', true), true)
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

test('detection case: no logs, Claude desktop only, or normal', () => {
  assert.equal(detectionCase(mockDetectedTools('nologs')), 'none')
  assert.equal(detectionCase([]), 'none')
  assert.equal(detectionCase(mockDetectedTools('desktoponly')), 'claude_desktop_only')
  assert.equal(detectionCase(mockDetectedTools(null)), 'normal')
  assert.equal(detectionCase(mockDetectedTools('nodesktop')), 'normal')
  // Codex logs do not change the Claude desktop-only case.
  const desktopAndCodex = mockDetectedTools('desktoponly').map(tool => tool.source === 'CodexCli' ? { ...tool, sessions: 2 } : tool)
  assert.equal(detectionCase(desktopAndCodex), 'claude_desktop_only')
  // Codex only: Claude has nothing to explain.
  const codexOnly = mockDetectedTools('nologs').map(tool => tool.source === 'CodexDesktop' ? { ...tool, sessions: 3 } : tool)
  assert.equal(detectionCase(codexOnly), 'normal')
})

test('preview fixtures for no logs and Claude desktop only', () => {
  const none = mockDetectedTools('nologs')
  assert.deepEqual(none.map(tool => tool.source), ['ClaudeCli', 'ClaudeDesktop', 'CodexCli', 'CodexDesktop'])
  assert.ok(none.every(tool => tool.sessions === 0 && tool.requests === 0 && tool.last_used_at === null))
  const desktopOnly = mockDetectedTools('desktoponly')
  assert.deepEqual(desktopOnly.filter(tool => tool.sessions > 0).map(tool => tool.source), ['ClaudeDesktop'])
})

test('quota sources explain each switch; the status line needs the CLI for desktop-only users', () => {
  const normal = quotaSources()
  assert.ok(normal.every(item => item.help.length > 0))
  assert.ok(normal.every(item => item.notes.length === 0))
  const statusline = normal.find(item => item.key === 'statusline')
  assert.match(statusline.help, /备份/)
  const claude = normal.find(item => item.key === 'claudeOfficial')
  assert.match(claude.help, /Anthropic/)
  assert.match(claude.help, /不保存/)
  assert.ok(!/Anthropic|Claude 登录/.test(normal.find(item => item.key === 'codexOfficial').help))
  const desktopOnly = quotaSources('claude_desktop_only').find(item => item.key === 'statusline')
  assert.deepEqual(desktopOnly.notes, [m().onboarding.statuslineNeedsCli])
  assert.deepEqual(quotaSources('none').find(item => item.key === 'statusline').notes, [])
})

test('setup summary is short on success and lists only failures', () => {
  const selection = { statusline: false, claudeOfficial: true, codexOfficial: true }
  const result = { claude: { state: 'updated', message: '' }, codex: { state: 'updated', message: '' } }
  assert.deepEqual(onboardingSummary(quotaOutcome(selection, result)), {
    text: '设置完成，额度会自动更新', tone: 'success',
  })
  const failed = onboardingSummary(quotaOutcome(selection, { ...result, claude: { state: 'error', message: '读取Claude 桌面版凭据被拒绝' } }))
  assert.equal(failed.tone, 'warning')
  assert.equal(failed.text, 'Claude 官方额度：读取Claude 桌面版凭据被拒绝')
})

test('notification line follows the permission state', () => {
  assert.deepEqual(notificationLine('authorized'), { text: '已允许发送通知', tone: 'success' })
  assert.deepEqual(notificationLine('denied'), { text: '未允许，可在系统设置 → 通知中开启', tone: 'warning' })
  assert.deepEqual(notificationLine('not_determined'), { text: '尚未允许发送通知', tone: 'neutral' })
  // Windows and the dev build cannot read the permission.
  assert.deepEqual(notificationLine('unknown'), { text: '无法读取通知权限', tone: 'neutral' })
})

test('tray place names the platform', () => {
  const mac = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'
  const windows = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
  assert.equal(trayPlace(mac), '菜单栏')
  assert.equal(trayPlace(windows), '任务栏右下角')
})
