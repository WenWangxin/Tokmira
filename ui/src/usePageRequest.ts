import { ref, shallowRef, onUnmounted } from 'vue'
import { cacheVersion, cacheVersionMatches, getPageCache, setPageCache } from './pageCache'

/** Keeps old data visible while refreshing and delays a cold-load skeleton. */
export function usePageRequest<T>(initialKey: string) {
  const data = shallowRef<T | null>(getPageCache<T>(initialKey) ?? null)
  const pending = ref(data.value === null)
  const loading = ref(false)
  const error = ref('')
  const reveal = ref(data.value === null)
  let timer: ReturnType<typeof setTimeout> | undefined
  let ticket = 0
  let disposed = false

  /** `keep` refreshes in place: the data on screen stays until the new result arrives. */
  async function load(key: string, request: () => Promise<T>, options: { keep?: boolean } = {}): Promise<T | null> {
    const current = ++ticket
    if (timer) clearTimeout(timer)
    const cached = getPageCache<T>(key)
    const keep = options.keep === true && data.value !== null
    if (!keep) {
      data.value = (cached ?? null)
      pending.value = cached === undefined
    }
    loading.value = false
    error.value = ''
    if (!keep && cached === undefined) {
      timer = setTimeout(() => { if (!disposed && current === ticket) loading.value = true }, 250)
    }
    const version = cacheVersion(key)
    try {
      const result = await request()
      if (!cacheVersionMatches(key, version)) return null
      setPageCache(key, result)
      if (!disposed && current === ticket) {
        data.value = result
        pending.value = false
        error.value = ''
      }
      return result
    } catch (cause) {
      if (!disposed && current === ticket && cached === undefined && !keep) error.value = cause instanceof Error ? cause.message : String(cause)
      return null
    } finally {
      if (!disposed && current === ticket) {
        if (timer) clearTimeout(timer)
        timer = undefined
        pending.value = false
        loading.value = false
      }
    }
  }

  function replace(key: string, value: T): void {
    setPageCache(key, value)
    data.value = value
    pending.value = false
    loading.value = false
  }
  onUnmounted(() => { disposed = true; ticket++; if (timer) clearTimeout(timer) })
  return { data, pending, loading, error, reveal, load, replace }
}
