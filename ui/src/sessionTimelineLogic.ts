import type { TimelineEntry } from './api'

export interface TimelineItem {
  entry: TimelineEntry
  sourceIndex: number
}

interface PromptGroup {
  prompt?: TimelineItem
  requests: TimelineItem[]
}

export function newestFirst(entries: readonly TimelineEntry[], limit: number): { shown: TimelineItem[]; hidden: number } {
  const groups: PromptGroup[] = []
  let group: PromptGroup = { requests: [] }
  let totalRequests = 0

  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const item = { entry: entries[index]!, sourceIndex: index }
    if (item.entry.kind === 'Request') {
      group.requests.push(item)
      totalRequests += 1
    } else {
      group.prompt = item
      groups.push(group)
      group = { requests: [] }
    }
  }
  if (group.requests.length) groups.push(group)

  const shown: TimelineItem[] = []
  let remaining = Math.max(0, Math.floor(limit))
  let shownRequests = 0
  let lastRequestEnd = 0
  for (const current of groups) {
    if (totalRequests && remaining === 0) break
    if (current.prompt) shown.push(current.prompt)
    const requests = current.requests.slice(0, remaining)
    shown.push(...requests)
    shownRequests += requests.length
    remaining -= requests.length
    if (requests.length) lastRequestEnd = shown.length
  }
  // Prompt-only groups older than the last visible request are outside the page.
  if (totalRequests) shown.length = lastRequestEnd

  return { shown, hidden: totalRequests - shownRequests }
}
