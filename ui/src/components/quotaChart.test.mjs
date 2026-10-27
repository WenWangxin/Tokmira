import assert from 'node:assert/strict'
import test from 'node:test'
import { placeMarkerLabels, quotaTicks, remainingQuotaColor } from './quotaChart.ts'

test('quota ticks have an integer, sufficient ceiling', () => {
  assert.deepEqual(quotaTicks(0), [0, 10, 20, 30, 40])
  assert.deepEqual(quotaTicks(40), [0, 10, 20, 30, 40])
  assert.deepEqual(quotaTicks(69), [0, 20, 40, 60, 80])
  assert.deepEqual(quotaTicks(70), [0, 20, 40, 60, 80])
  assert.deepEqual(quotaTicks(100), [0, 25, 50, 75, 100])
  for (let peak = 0; peak <= 100; peak++) {
    const ticks = quotaTicks(peak)
    assert.equal(ticks.length, 5)
    assert.ok(ticks.every(Number.isInteger))
    assert.ok(ticks.at(-1) >= peak)
  }
})

test('larger markers keep labels, smaller labels move below or disappear', () => {
  assert.deepEqual(placeMarkerLabels([
    { x: 100, y: 60, delta: 3 },
    { x: 101, y: 60, delta: 21.7 },
    { x: 102, y: 60, delta: 2 },
  ]), [80, 49, null])
})

test('remaining quota colour runs from red through orange to green', () => {
  assert.equal(remainingQuotaColor(100), 'color-mix(in oklch, var(--success) 100%, var(--warning))')
  assert.equal(remainingQuotaColor(75), 'color-mix(in oklch, var(--success) 50%, var(--warning))')
  assert.equal(remainingQuotaColor(50), 'color-mix(in oklch, var(--success) 0%, var(--warning))')
  assert.equal(remainingQuotaColor(25), 'color-mix(in oklch, var(--warning) 50%, var(--error))')
  assert.equal(remainingQuotaColor(0), 'color-mix(in oklch, var(--warning) 0%, var(--error))')
  assert.equal(remainingQuotaColor(-12), remainingQuotaColor(0))
  assert.equal(remainingQuotaColor(140), remainingQuotaColor(100))
})
