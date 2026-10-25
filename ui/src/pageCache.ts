import type { Tool } from './api'

export type PageCacheGroup = 'overview' | 'advice' | 'usageWindows' | 'usageDetail' | 'sessions' | 'sessionDetail' | 'settings'

// Keys include every backend parameter. A window id can contain punctuation, so encode parts.
export function pageCacheKey(group: PageCacheGroup, ...parts: (string | number)[]): string {
  return [group, ...parts.map(part => encodeURIComponent(String(part)))].join(':')
}

export function keysToInvalidate(keys: readonly string[], groups: readonly PageCacheGroup[]): string[] {
  return keys.filter(key => groups.some(group => key === group || key.startsWith(`${group}:`)))
}

const values = new Map<string, unknown>()
const versions = new Map<string, number>()

export function getPageCache<T>(key: string): T | undefined { return values.get(key) as T | undefined }
export function setPageCache<T>(key: string, value: T): void { values.set(key, value) }
export function cacheVersion(key: string): number {
  if (!versions.has(key)) versions.set(key, 0)
  return versions.get(key) ?? 0
}
export function cacheVersionMatches(key: string, version: number): boolean { return cacheVersion(key) === version }
export function invalidatePageCache(...groups: PageCacheGroup[]): void {
  for (const key of keysToInvalidate([...new Set([...values.keys(), ...versions.keys()])], groups)) {
    values.delete(key)
    versions.set(key, cacheVersion(key) + 1)
  }
}

export const pageSelection = {
  advice: { tool: null as Tool | null, period: 7 as 7 | 30 },
  usage: { tool: null as Tool | null, windowIds: {} as Partial<Record<Tool, string>>, dimension: 'sessions' as 'sessions' | 'projects' | 'models' | 'subagents' },
  sessions: { query: '', toolFilter: 'all' as 'all' | Tool, visibleCount: 50 },
}
