import assert from 'node:assert/strict'
import test from 'node:test'
import { easeOutCubic, formatAnimatedNumber, numericValue } from './motionNumber.ts'

test('number interpolation keeps target precision, grouping, prefix and suffix', () => {
  assert.equal(formatAnimatedNumber(0, '12.50%', 0), '0.00%')
  assert.equal(formatAnimatedNumber(0, '12.50%', 1), '12.50%')
  assert.equal(formatAnimatedNumber(0, '$1,284.40', 1), '$1,284.40')
  assert.equal(formatAnimatedNumber(0, '1.2 万', 1), '1.2 万')
  assert.equal(formatAnimatedNumber(0, '—', 0.5), '—')
})

test('ease out is bounded and value parsing does not accept labels', () => {
  assert.equal(easeOutCubic(-1), 0)
  assert.equal(easeOutCubic(1), 1)
  assert.ok(easeOutCubic(0.5) > 0.5)
  assert.equal(numericValue('1,284.40'), 1284.4)
  assert.equal(numericValue('已重置'), null)
})
