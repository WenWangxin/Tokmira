import test from 'node:test'
import assert from 'node:assert/strict'
import { outcomeMessage } from './sessionOpenLogic.ts'

test('fill outcomes say whether the command was typed or copied', () => {
  assert.deepEqual(outcomeMessage('filled', '/compact'), { text: '已在会话里填入 /compact，确认后按回车发送', tone: 'success' })
  assert.match(outcomeMessage('focused_copied', '/compact').text, /已复制/)
  assert.equal(outcomeMessage('focused').text, '已切换到这个会话')
  assert.equal(outcomeMessage('app_copied', '/compact').tone, 'info')
  assert.match(outcomeMessage('app_copied', '/compact').text, /分不清是哪个标签页/)
  assert.equal(outcomeMessage('resumed').text, '这个会话已结束，已在新窗口中重新打开')
  assert.equal(outcomeMessage('not_found').tone, 'warning')
  assert.match(outcomeMessage('not_found', '/model sonnet').text, /\/model sonnet 已复制/)
})
