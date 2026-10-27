import assert from 'node:assert/strict'
import test from 'node:test'
import { overallTone, sourceRows, timeAgo } from './sourceStatus.ts'

const now = Date.parse('2026-10-25T14:00:00Z')
const base = {
  claudeOfficialEnabled: true,
  codexOfficialEnabled: true,
  recorder: { statusline_installed: false, statusline_modified: false, last_statusline_record: null },
  times: { claude_official: '2026-10-25T13:58:00Z', codex_official: '2026-10-25T13:59:30Z', codex_log: '2026-10-25T11:00:00Z' },
  claudeNeedsAuthorization: false,
  lastResult: null,
  now,
}
const byKey = rows => Object.fromEntries(rows.map(row => [row.key, row]))

test('time ago reads naturally', () => {
  assert.equal(timeAgo('2026-10-25T13:59:40Z', now), '刚刚')
  assert.equal(timeAgo('2026-10-25T13:55:00Z', now), '5 分钟前')
  assert.equal(timeAgo('2026-10-25T11:00:00Z', now), '3 小时前')
})

test('healthy sources show when they last delivered', () => {
  const rows = byKey(sourceRows(base))
  assert.deepEqual([rows.claudeOfficial.status, rows.claudeOfficial.tone], ['2 分钟前更新', 'ok'])
  assert.deepEqual([rows.codexOfficial.status, rows.codexOfficial.tone], ['刚刚更新', 'ok'])
  assert.equal(rows.codexLog.status, '3 小时前记录')
  assert.deepEqual([rows.statusline.status, rows.statusline.action], ['未安装', 'settings'])
  assert.equal(overallTone(sourceRows(base), ''), 'success')
})

test('authorization, failures and switched-off sources', () => {
  const rows = byKey(sourceRows({
    ...base,
    claudeNeedsAuthorization: true,
    codexOfficialEnabled: false,
    lastResult: { claude: { state: 'needs_authorization', message: '' }, codex: { state: 'off', message: '' } },
  }))
  assert.deepEqual([rows.claudeOfficial.tone, rows.claudeOfficial.action], ['warning', 'authorize'])
  assert.deepEqual([rows.codexOfficial.status, rows.codexOfficial.action], ['未开启', 'settings'])

  const failed = byKey(sourceRows({ ...base, lastResult: { claude: { state: 'error', message: '无法连接 Anthropic' }, codex: { state: 'recent', message: '' } } }))
  assert.deepEqual([failed.claudeOfficial.status, failed.claudeOfficial.tone], ['无法连接 Anthropic', 'warning'])
  assert.equal(failed.codexOfficial.tone, 'ok')
  assert.equal(overallTone(Object.values(failed), ''), 'warning')
  assert.equal(overallTone(sourceRows(base), '加载失败'), 'warning')
})

test('status line recorder states and missing timestamps', () => {
  const waiting = byKey(sourceRows({ ...base, recorder: { statusline_installed: true, statusline_modified: false, last_statusline_record: null }, times: null }))
  assert.equal(waiting.statusline.status, '尚未收到（仅命令行版有效）')
  assert.equal(waiting.claudeOfficial.status, '暂无数据')
  assert.equal(waiting.codexLog.status, '暂无额度记录')
  const modified = byKey(sourceRows({ ...base, recorder: { statusline_installed: true, statusline_modified: true, last_statusline_record: null } }))
  assert.equal(modified.statusline.tone, 'warning')
})
