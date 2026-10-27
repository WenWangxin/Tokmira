import { ref } from 'vue'
import type { RefreshResult, QuotaState } from './api'

/** Bumped when local usage changed; mounted pages watch it and refresh in place. */
export const usageRevision = ref(0)
export const settingsRevision = ref(0)

/**
 * True after a background Claude query could not read the login without asking.
 * The dashboard then offers a button that asks once; "always allow" there lets
 * later launches update on their own.
 */
export const claudeNeedsAuthorization = ref(false)

/** The latest official quota query result per tool, for the source status panel. */
export const lastQuotaResult = ref<RefreshResult | null>(null)

/** Records a quota refresh result and updates {@link claudeNeedsAuthorization}. */
export function noteQuotaResult(result: RefreshResult) {
  claudeNeedsAuthorization.value = nextAuthorizationState(claudeNeedsAuthorization.value, result.claude.state)
  lastQuotaResult.value = result
}

/** “需要授权” sets the flag, a successful query clears it, anything else keeps it. */
export function nextAuthorizationState(current: boolean, claude: QuotaState): boolean {
  if (claude === 'needs_authorization') return true
  if (claude === 'updated') return false
  return current
}

export const QUOTA_POLL_MS = 60_000
export const REFRESH_GAP_MS = 1_000

/**
 * Whether to ask the backend for official quotas now. Only while the window is
 * visible; the backend still applies its own minimum interval and rate-limit
 * backoff, and does nothing unless the user switched a query on.
 */
export function quotaPollDue(now: number, lastPollAt: number | null, visible: boolean, intervalMs = QUOTA_POLL_MS): boolean {
  return visible && (lastPollAt === null || now - lastPollAt >= intervalMs)
}

/** Delay before the next page refresh, so bursts of changes refresh at most once a second. */
export function refreshDelay(now: number, lastRefreshAt: number | null, gapMs = REFRESH_GAP_MS): number {
  return lastRefreshAt === null ? 0 : Math.max(0, lastRefreshAt + gapMs - now)
}

interface RealtimeDeps {
  listen: (handler: () => void) => Promise<() => void>
  refreshQuotas: () => Promise<unknown>
  visible: () => boolean
  now?: () => number
}

/** Wires the backend's change events and the visible-window quota poll. Returns a stop function. */
export function startRealtime({ listen, refreshQuotas, visible, now = Date.now }: RealtimeDeps): () => void {
  let lastPollAt: number | null = null
  let lastRefreshAt: number | null = null
  let refreshTimer: ReturnType<typeof setTimeout> | undefined
  let polling = false
  let stopped = false

  const bump = () => {
    if (refreshTimer) return
    refreshTimer = setTimeout(() => {
      refreshTimer = undefined
      lastRefreshAt = now()
      usageRevision.value += 1
    }, refreshDelay(now(), lastRefreshAt))
  }

  // New quota snapshots land in the app's data folder, which the backend watches,
  // so a successful query comes back as a change event.
  const poll = async (force = false) => {
    if (stopped || polling || !(force ? visible() : quotaPollDue(now(), lastPollAt, visible()))) return
    polling = true
    lastPollAt = now()
    try { await refreshQuotas() } catch { /* The dashboard shows query errors on manual refresh. */ }
    finally { polling = false }
  }

  let unlisten: (() => void) | undefined
  void listen(() => { bump(); void poll(true) }).then(stop => { if (stopped) stop(); else unlisten = stop })
  const interval = setInterval(() => { void poll() }, QUOTA_POLL_MS / 4)
  const onVisibility = () => { if (visible()) { bump(); void poll() } }
  document.addEventListener('visibilitychange', onVisibility)
  void poll()

  return () => {
    stopped = true
    unlisten?.()
    clearInterval(interval)
    if (refreshTimer) clearTimeout(refreshTimer)
    document.removeEventListener('visibilitychange', onVisibility)
  }
}
