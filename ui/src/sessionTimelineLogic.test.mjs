import test from 'node:test'
import assert from 'node:assert/strict'
import { newestFirst } from './sessionTimelineLogic.ts'

const prompt = at => ({ kind: 'Prompt', at, gap_minutes_from_previous_prompt: null })
const request = (index, at) => ({ kind: 'Request', index, at })
const labels = shown => shown.map(({ entry }) => entry.kind === 'Prompt' ? `P${entry.at}` : `#${entry.index}`)

test('reverses prompt groups and keeps prompt titles above their requests', () => {
  const entries = [prompt('1'), request(7, '2'), prompt('3'), request(9, '4')]
  assert.deepEqual(labels(newestFirst(entries, 50).shown), ['P3', '#9', 'P1', '#7'])
  assert.deepEqual(entries.map(entry => entry.at), ['1', '2', '3', '4'])
})

test('reverses requests within a prompt group without renumbering them', () => {
  const entries = [prompt('1'), request(3, '2'), request(8, '3'), request(21, '4')]
  assert.deepEqual(labels(newestFirst(entries, 50).shown), ['P1', '#21', '#8', '#3'])
})

test('puts requests before the first prompt in an untitled group at the bottom', () => {
  const entries = [request(1, '1'), request(2, '2'), prompt('3'), request(3, '4')]
  assert.deepEqual(labels(newestFirst(entries, 50).shown), ['P3', '#3', '#2', '#1'])
})

test('cuts off inside a group while keeping its prompt and revealing older requests next', () => {
  const entries = [prompt('1'), request(1, '2'), prompt('3'), request(2, '4'), request(3, '5'), request(4, '6')]
  const first = newestFirst(entries, 2)
  assert.deepEqual(labels(first.shown), ['P3', '#4', '#3'])
  assert.equal(first.hidden, 2)
  const more = newestFirst(entries, 3)
  assert.deepEqual(labels(more.shown), ['P3', '#4', '#3', '#2'])
  assert.equal(more.hidden, 1)
  assert.deepEqual(more.shown.slice(0, first.shown.length).map(item => item.sourceIndex), first.shown.map(item => item.sourceIndex))
})

test('places prompt-only groups by time and hides them if older than all visible requests', () => {
  const entries = [prompt('1'), prompt('2'), request(1, '3'), prompt('4'), prompt('5'), request(2, '6'), prompt('7')]
  assert.deepEqual(labels(newestFirst(entries, 1).shown), ['P7', 'P5', '#2'])
  assert.deepEqual(labels(newestFirst(entries, 2).shown), ['P7', 'P5', '#2', 'P4', 'P2', '#1'])
  assert.equal(newestFirst(entries, 2).hidden, 0)
})

test('shows all requests when limit exceeds total and reports no hidden requests', () => {
  const entries = [request(1, '1'), prompt('2'), request(2, '3')]
  assert.deepEqual(labels(newestFirst(entries, 50).shown), ['P2', '#2', '#1'])
  assert.equal(newestFirst(entries, 50).hidden, 0)
})

test('keeps a timeline made only of prompts', () => {
  assert.deepEqual(labels(newestFirst([prompt('1'), prompt('2')], 50).shown), ['P2', 'P1'])
})

test('handles an empty timeline', () => {
  assert.deepEqual(newestFirst([], 50), { shown: [], hidden: 0 })
})
