import assert from 'node:assert/strict'
import test from 'node:test'
import { lastCheckedLine, progressLine, updateActionLabel } from './updateLogic.ts'

test('last check time reads as today or a dated day', () => {
  const now = new Date(2026, 9, 27, 18, 0)
  assert.equal(lastCheckedLine(null, now), '尚未检查过')
  assert.equal(lastCheckedLine('not a date', now), '尚未检查过')
  assert.equal(lastCheckedLine(new Date(2026, 9, 27, 17, 3).toISOString(), now), '上次检查：今天 17:03')
  assert.equal(lastCheckedLine(new Date(2026, 9, 25, 9, 5).toISOString(), now), '上次检查：10 月 25 日 09:05')
})

test('download progress shows a percentage only when the size is known', () => {
  const mb = 1024 * 1024
  assert.equal(progressLine(3.2 * mb, 12 * mb), '已下载 3.2 / 12.0 MB（26%）')
  assert.equal(progressLine(12 * mb, 12 * mb), '已下载 12.0 / 12.0 MB（100%）')
  assert.equal(progressLine(5 * mb, null), '已下载 5.0 MB')
})

test('the dialog button follows the install stage', () => {
  assert.equal(updateActionLabel('ready'), '下载并安装')
  assert.equal(updateActionLabel('installing'), '正在安装…')
  assert.equal(updateActionLabel('installed'), '重启以完成更新')
})
