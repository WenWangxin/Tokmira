import assert from 'node:assert/strict'
import test from 'node:test'
import { mockDetectedTools } from './mocks/onboarding.ts'
import { m } from './i18n/index.ts'
import {
  detectionCase,
  quotaSources,
  notificationLine,
  trayPlace,
  toolGroups,
  initialOnboardingChoices,
  installOutcome,
  onboardingExitTarget,
  onboardingSummary,
  outcomeTone,
  previewOnboardingForced,
  quotaOutcome,
} from './onboardingLogic.ts'

test('quota sources come from the registry, one per switch', () => {
  assert.deepEqual(quotaSources().map(item => item.key), ['statusline', 'claudeOfficial', 'codexOfficial', 'deepseekOfficial'])
  assert.equal(quotaSources()[0].title, 'Claude Code 状态栏记录')
})

test('each tool shows its command line and desktop apps on one row', () => {
  const groups = toolGroups([
    { source: 'ClaudeCli', sessions: 34, requests: 900, plan: null },
    { source: 'ClaudeDesktop', sessions: 0, requests: 0, plan: null },
    { source: 'CodexCli', sessions: 4, requests: 60, plan: 'plus' },
    { source: 'CodexDesktop', sessions: 15, requests: 40, plan: 'plus' },
    { source: 'DeepSeek', sessions: 1, requests: 1, plan: null, install: { desktop: true, cli: false } },
  ])
  assert.deepEqual(groups.map(group => group.tool), ['Claude', 'Codex', 'DeepSeek'])
  const [claude, codex, deepseek] = groups
  assert.deepEqual(claude.variants, [
    { label: '命令行', found: true, detail: '34 个会话' },
    { label: '桌面版', found: false, detail: '未检测到' },
  ])
  assert.equal(claude.hint, '额度需开启记录器或官方查询')
  assert.deepEqual(claude.pill, { text: 'token 精确', tone: 'success' })
  assert.deepEqual(codex.pill, { text: 'token 与额度精确', tone: 'success' })
  assert.deepEqual(deepseek.variants, [
    { label: '命令行', found: false, detail: '未检测到' },
    { label: '桌面版', found: true, detail: '1 个会话' },
  ])
})

test('DeepSeek dots follow what is installed, since one log serves both apps', () => {
  const group = installs => toolGroups([{ source: 'DeepSeek', sessions: 3, requests: 9, plan: null, install: installs }])[2]
  // Both installed: the sessions go with the desktop app, the CLI is just installed.
  assert.deepEqual(group({ desktop: true, cli: true }).variants.map(item => item.detail), ['已安装', '3 个会话'])
  // Only the CLI: its sessions are the CLI's.
  assert.deepEqual(group({ desktop: false, cli: true }).variants.map(item => [item.found, item.detail]), [[true, '3 个会话'], [false, '未检测到']])
  // Logs but nothing found installed (a moved app): still counted, under desktop.
  assert.deepEqual(group({ desktop: false, cli: false }).variants.map(item => item.found), [false, true])
  assert.deepEqual(toolGroups([])[2].pill, { text: '未检测到', tone: 'neutral' })
})

test('first run defaults; reopening keeps current switches', () => {
  const enabled = { claude_official_query: true, codex_app_server: true, deepseek_balance_query: true }
  const disabled = { claude_official_query: false, codex_app_server: false, deepseek_balance_query: false }
  const off = { statusline: true, claudeOfficial: false, codexOfficial: false, deepseekOfficial: false }
  // First run: status line on, every official query off, whatever the stored settings say.
  assert.deepEqual(initialOnboardingChoices(false, disabled, false), off)
  assert.deepEqual(initialOnboardingChoices(false, enabled, false), off)
  assert.deepEqual(initialOnboardingChoices(true, enabled, false), {
    statusline: false,
    claudeOfficial: true,
    codexOfficial: true,
    deepseekOfficial: true,
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
  const deepseek = { ...result, deepseek: { state: 'error', message: 'DeepSeek 账号登录已失效，请在 Harness 里重新登录' } }
  assert.deepEqual(quotaOutcome({ statusline: false, claudeOfficial: false, codexOfficial: false, deepseekOfficial: true }, deepseek).map(line => line.text),
    ['DeepSeek 余额：DeepSeek 账号登录已失效，请在 Harness 里重新登录'])
})

test('preview query forces the guide only in the browser preview', () => {
  assert.equal(previewOnboardingForced('?onboarding=1', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=nodesktop', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=nologs', true), true)
  assert.equal(previewOnboardingForced('?onboarding=1&mock=desktoponly', true), true)
  assert.equal(previewOnboardingForced('', true), false)
  assert.equal(previewOnboardingForced('?onboarding=1', false), false)
})

test('calibration link navigates only after onboarding completes', () => {
  assert.equal(onboardingExitTarget(false, true), null)
  assert.equal(onboardingExitTarget(true, false), null)
  assert.equal(onboardingExitTarget(true, true), '/settings?section=calibration&tool=Claude')
})

test('preview fixtures cover both tools and a missing Claude desktop log', () => {
  const detected = mockDetectedTools(null)
  assert.equal(detected.length, 5)
  assert.ok(detected.filter(tool => tool.source.startsWith('Claude')).some(tool => tool.sessions > 0))
  assert.ok(detected.filter(tool => tool.source.startsWith('Codex')).every(tool => tool.sessions > 0 && tool.plan === 'plus'))
  assert.equal(detected.find(tool => tool.source.startsWith('Claude'))?.plan ?? null, null)
  const desktop = mockDetectedTools('nodesktop').find(tool => tool.source === 'ClaudeDesktop')
  assert.ok(desktop)
  assert.equal(desktop.sessions, 0)
  assert.equal(desktop.requests, 0)
  assert.equal(desktop.last_used_at, null)
  assert.deepEqual(toolGroups(mockDetectedTools('nodesktop'))[0].variants[1], { label: '桌面版', found: false, detail: '未检测到' })
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
  assert.deepEqual(none.map(tool => tool.source), ['ClaudeCli', 'ClaudeDesktop', 'CodexCli', 'CodexDesktop', 'DeepSeek'])
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
