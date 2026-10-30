<script setup lang="ts">
import { clock, compactText, formatInteger, listJoin, messages, m } from '../i18n/index.ts'
const t = messages
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { CircleAlert, Database, Search, SquareArrowOutUpRight, X } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import PageSkeleton from '../components/PageSkeleton.vue'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usePageRequest } from '../usePageRequest'
import { usageRevision } from '../realtime'
import { openAndReport } from '../sessionOpen'
import { newestFirst } from '../sessionTimelineLogic'
import {
  getSessionDetail, getSessions,
  type SessionDetail, type SessionRow, type Source, type TimelineEntry, type TokenCounts, type Tool,
} from '../api'

defineOptions({ inheritAttrs: false })

const route = useRoute()
const router = useRouter()
const listKey = pageCacheKey('sessions', 7)
const listRequest = usePageRequest<SessionRow[]>(listKey)
const rows = computed(() => listRequest.data.value ?? [])
const { pending, loading, error, reveal } = listRequest
const query = ref(pageSelection.sessions.query)
const toolFilter = ref<'all' | Tool>(pageSelection.sessions.toolFilter)
const visibleCount = ref(pageSelection.sessions.visibleCount)
const detail = ref<SessionDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref('')
const requestLimit = ref(50)
let detailTicket = 0
let detailTimer: ReturnType<typeof setTimeout> | undefined
const detailPending = ref(false)
let previousContentOverflow = ''
let contentLocked = false

function failure(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause)
}
function parseTool(value: unknown): Tool | null {
  const text = Array.isArray(value) ? value[0] : value
  if (text === 'Claude' || text === 'claude') return 'Claude'
  if (text === 'Codex' || text === 'codex') return 'Codex'
  return null
}
const routeSession = computed(() => {
  const tool = parseTool(route.query.tool)
  const id = typeof route.query.id === 'string' ? route.query.id : ''
  return tool && id ? { tool, id } : null
})

function setContentLock(selected: boolean) {
  const content = document.querySelector<HTMLElement>('.content')
  if (!content) return
  if (selected) {
    if (!contentLocked) previousContentOverflow = content.style.overflowY
    content.style.overflowY = 'hidden'
    contentLocked = true
  } else if (contentLocked) {
    content.style.overflowY = previousContentOverflow
    contentLocked = false
  }
}

async function load(keep = false) {
  await listRequest.load(listKey, () => getSessions(7), { keep })
}
async function loadDetail(tool: Tool, sessionId: string) {
  const ticket = ++detailTicket
  if (detailTimer) clearTimeout(detailTimer)
  const key = pageCacheKey('sessionDetail', tool, sessionId)
  const cached = getPageCache<SessionDetail>(key)
  detail.value = cached ?? null
  detailPending.value = cached === undefined
  detailLoading.value = false
  detailError.value = ''
  if (cached === undefined) detailTimer = setTimeout(() => { if (ticket === detailTicket) detailLoading.value = true }, 250)
  const version = cacheVersion(key)
  try {
    const result = await getSessionDetail(tool, sessionId)
    if (!cacheVersionMatches(key, version)) return
    setPageCache(key, result)
    if (ticket === detailTicket) detail.value = result
  } catch (cause) {
    if (ticket === detailTicket && cached === undefined) detailError.value = failure(cause)
  } finally {
    if (ticket === detailTicket) {
      if (detailTimer) clearTimeout(detailTimer)
      detailPending.value = false
      detailLoading.value = false
    }
  }
}
watch(usageRevision, () => {
  void load(true)
  const open = routeSession.value
  if (open) void loadDetail(open.tool, open.id)
})
watch(routeSession, (selected) => {
  requestLimit.value = 50
  setContentLock(Boolean(selected))
  if (!selected) {
    detailTicket += 1
    if (detailTimer) clearTimeout(detailTimer)
    detailPending.value = false
    detail.value = null
    detailError.value = ''
    detailLoading.value = false
    return
  }
  void loadDetail(selected.tool, selected.id)
}, { immediate: true })
onMounted(() => setContentLock(Boolean(routeSession.value)))
onUnmounted(() => {
  setContentLock(false)
  if (detailTimer) clearTimeout(detailTimer)
})
onMounted(load)
function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape' && routeSession.value) closeSession()
}
onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))

const sorted = computed(() => [...rows.value].sort((a, b) =>
  b.weekly_delta - a.weekly_delta || b.estimated_cost_usd - a.estimated_cost_usd
  || a.tool.localeCompare(b.tool) || a.session_id.localeCompare(b.session_id)))
const filtered = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return sorted.value.filter(row => {
    if (toolFilter.value !== 'all' && row.tool !== toolFilter.value) return false
    if (!needle) return true
    return (row.title ?? '').toLowerCase().includes(needle)
      || row.project_name.toLowerCase().includes(needle)
      || row.display_name.toLowerCase().includes(needle)
      || row.session_id.toLowerCase().startsWith(needle)
  })
})
const visibleRows = computed(() => filtered.value.slice(0, visibleCount.value))
const quotaNote = computed(() => {
  const names = (['Claude', 'Codex'] as const).filter(tool =>
    filtered.value.some(row => row.tool === tool && !row.has_quota_data))
  return names.length ? m().sessions.quotaNote(listJoin(names)) : ''
})
watch([query, toolFilter], () => {
  visibleCount.value = 50
  pageSelection.sessions.query = query.value
  pageSelection.sessions.toolFilter = toolFilter.value
})
watch(visibleCount, count => { pageSelection.sessions.visibleCount = count })
watch([filtered, routeSession], () => {
  const selected = routeSession.value
  if (!selected) return
  const index = filtered.value.findIndex(row => row.tool === selected.tool && row.session_id === selected.id)
  if (index >= visibleCount.value) visibleCount.value = index + 1
})

function openSession(row: SessionRow) {
  const location = { path: '/sessions', query: { tool: row.tool, id: row.session_id } }
  if (routeSession.value) router.replace(location)
  else router.push(location)
}
const opening = ref(false)
async function openDetailSession() {
  const value = detail.value
  if (!value || opening.value) return
  opening.value = true
  try {
    await openAndReport({ tool: value.tool, source: value.source, session_id: value.session_id, project: value.project })
  } finally {
    opening.value = false
  }
}

function closeSession() {
  router.replace({ path: '/sessions' })
}
function selected(row: SessionRow) {
  return routeSession.value?.tool === row.tool && routeSession.value.id === row.session_id
}
const sourceLabel = (source: Source) => ({
  ClaudeCli: m().sources.label.ClaudeCli, ClaudeDesktop: m().sources.label.ClaudeDesktop,
  CodexCli: m().sources.label.CodexCli, CodexDesktop: m().sources.label.CodexDesktop, Unknown: m().sources.label.Unknown,
})[source]
const sourcePhrase = (source: Source) => ({
  ClaudeCli: m().sources.phrase.ClaudeCli, ClaudeDesktop: m().sources.phrase.ClaudeDesktop,
  CodexCli: m().sources.phrase.CodexCli, CodexDesktop: m().sources.phrase.CodexDesktop, Unknown: m().sources.label.Unknown,
})[source]
const tokens = (value: number) => compactText(value)
const hitText = (value: number | null) => value === null ? '—' : `${(value * 100).toFixed(0)}%`
const quotaText = (value: number, available: boolean) => available ? `${value.toFixed(1)}%` : '—'
const grouped = (value: number) => formatInteger(value)
function tokenLine(tool: Tool, item: TokenCounts) {
  const read = grouped(item.cache_read)
  const output = grouped(item.output)
  if (tool === 'Codex') return m().sessions.tokensCodex(grouped(item.input), read, output)
  const write = item.cache_write_5m + item.cache_write_1h + item.cache_write_unspecified
  return m().sessions.tokensClaude(grouped(write), read, output)
}
function clockText(value: string) {
  return clock(value, true)
}
function promptText(entry: Extract<TimelineEntry, { kind: 'Prompt' }>) {
  const gap = entry.gap_minutes_from_previous_prompt
  const text = m().sessions.promptAt(clockText(entry.at))
  return gap !== null && gap >= 30 ? `${text}${m().sessions.sinceLast(gap)}` : text
}
function deltaText(value: number) {
  if (Math.abs(value).toFixed(1) === '0.0') return ''
  return value > 0 ? `+${value.toFixed(1)}%` : `${value.toFixed(1)}%`
}
const timeline = computed(() => newestFirst(detail.value?.entries ?? [], requestLimit.value))
const visibleEntries = computed(() => timeline.value.shown)
const hiddenRequests = computed(() => timeline.value.hidden)
const listedSession = computed(() => {
  const selected = routeSession.value
  if (!selected) return null
  return rows.value.find(row => row.tool === selected.tool && row.session_id === selected.id) ?? null
})
const detailTitle = computed(() => detail.value?.title || detail.value?.display_name || listedSession.value?.title || listedSession.value?.display_name || routeSession.value?.id || m().sessions.fallbackTitle)
const detailSubtitle = computed(() => {
  const source = detail.value ?? listedSession.value
  if (!source) return ''
  const project = source.project_name || m().common.unknownProject
  return `${project} · ${sourcePhrase(source.source)} · ${source.model || '—'}`
})
</script>

<template>
  <div class="dashboard-page sessions-page" :class="{ 'no-entry-reveal': !reveal }">
    <header class="dashboard-header">
      <div>
        <h1>{{ t.pages.sessions.title }}</h1>
        <p>{{ t.sessions.subtitle }}</p>
      </div>
      <div class="sessions-toolbar">
        <label class="session-search">
          <Search :size="16" aria-hidden="true" />
          <input v-model="query" type="search" :placeholder="t.sessions.search" :aria-label="t.sessions.search" />
        </label>
        <div v-selection-slide class="segmented" role="group" :aria-label="t.sessions.filter">
          <button type="button" :class="{ active: toolFilter === 'all' }" :aria-pressed="toolFilter === 'all'" @click="toolFilter = 'all'">{{ t.sessions.all }}</button>
          <button v-for="tool in (['Claude', 'Codex'] as const)" :key="tool" type="button" :class="{ active: toolFilter === tool }" :aria-pressed="toolFilter === tool" @click="toolFilter = tool">{{ tool }}</button>
        </div>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="sessions" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !listRequest.data.value" class="center-state"><CircleAlert :size="28" /><strong>{{ t.sessions.loadFailed }}</strong><p>{{ error }}</p><button type="button" @click="load()">{{ t.common.retry }}</button></div>
    <div v-else-if="!rows.length" class="center-state"><Database :size="28" /><strong>{{ t.sessions.empty }}</strong><p>{{ t.sessions.emptyBody }}</p></div>
    <div v-else-if="!filtered.length" class="center-state"><Search :size="28" /><strong>{{ t.sessions.noMatch }}</strong><p>{{ t.sessions.noMatchBody }}</p></div>
    <section v-else class="card sessions-table content-reveal" :aria-label="t.sessions.list">
      <div class="session-head"><span>{{ t.common.source }}</span><span>{{ t.common.session }}</span><span>{{ t.common.requests }}</span><span>{{ t.common.token }}</span><span>{{ t.common.cacheHit }}</span><span>{{ t.common.quota }}</span></div>
      <button v-for="row in visibleRows" :key="`${row.tool}-${row.session_id}`" type="button" class="session-line" :class="{ active: selected(row) }" :aria-current="selected(row) ? 'true' : undefined" @click="openSession(row)">
        <span class="row-source"><span class="row-icon"><img v-if="row.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span>{{ sourceLabel(row.source) }}</span>
        <span class="session-name"><strong :title="row.title || row.display_name">{{ row.title || row.display_name }}</strong><small v-if="row.title && row.project_name">{{ row.project_name }}</small></span>
        <span class="session-metric" :data-label="t.common.requests">{{ formatInteger(row.requests) }}</span>
        <span class="session-metric" :data-label="t.common.token">{{ tokens(row.total_tokens) }}</span>
        <span class="session-metric" :data-label="t.common.cacheHit" :class="{ 'error-text': row.cache_hit_rate !== null && row.cache_hit_rate < 0.5 }">{{ hitText(row.cache_hit_rate) }}</span>
        <span class="session-metric session-quota" :data-label="t.common.quota">{{ quotaText(row.weekly_delta, row.has_quota_data) }}</span>
      </button>
      <button v-if="filtered.length > visibleCount" type="button" class="sessions-more" @click="visibleCount += 50">{{ t.common.showMore }}</button>
      <p v-if="quotaNote" class="sessions-quota-note">{{ quotaNote }}</p>
    </section>

    <Transition name="drawer-mask"><button v-if="routeSession" type="button" class="session-mask" :aria-label="t.sessions.closeDetail" @click="closeSession" /></Transition>
    <Transition name="drawer"><aside v-if="routeSession" class="session-drawer" role="dialog" aria-modal="true" aria-labelledby="session-detail-title" :style="{ '--session-tool': (detail?.tool ?? routeSession.tool) === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
        <div class="session-drawer-head">
          <div>
            <h2 id="session-detail-title">{{ detailTitle }}</h2>
            <p v-if="detailSubtitle">{{ detailSubtitle }}</p>
          </div>
          <div class="session-drawer-actions">
            <button v-if="detail && !detailPending" type="button" class="session-open" :title="t.sessions.openSessionHint" :disabled="opening" @click="openDetailSession"><SquareArrowOutUpRight :size="15" aria-hidden="true" />{{ t.sessions.openSession }}</button>
            <button type="button" class="session-close" :aria-label="t.common.close" @click="closeSession"><X :size="18" /></button>
          </div>
        </div>
        <PageSkeleton v-if="detailPending && detailLoading" kind="sessionDetail" />
        <div v-else-if="detailPending" class="session-drawer-status" />
        <div v-else-if="detailError" class="session-drawer-status"><CircleAlert :size="24" /><strong>{{ t.sessions.detailFailed }}</strong><p>{{ detailError }}</p></div>
        <template v-else-if="detail">
          <div class="session-stats">
            <div class="session-stat"><small>{{ t.common.requests }}</small><strong>{{ formatInteger(detail.requests) }}</strong></div>
            <div class="session-stat"><small>{{ t.common.token }}</small><strong>{{ tokens(detail.total_tokens) }}</strong></div>
            <div class="session-stat"><small>{{ t.common.cacheHit }}</small><strong :class="{ 'error-text': detail.cache_hit_rate !== null && detail.cache_hit_rate < 0.5 }">{{ hitText(detail.cache_hit_rate) }}</strong></div>
            <div class="session-stat"><small>{{ t.common.quota }}</small><strong class="session-stat-quota">{{ quotaText(detail.weekly_delta, detail.has_quota_data) }}</strong></div>
          </div>
          <div class="session-scroll">
            <h3>{{ t.sessions.detail }}</h3>
            <ol class="timeline">
              <li v-for="{ entry, sourceIndex } in visibleEntries" :key="sourceIndex" class="timeline-item">
                <template v-if="entry.kind === 'Prompt'">
                  <span class="timeline-dot timeline-dot-prompt" />
                  <p class="timeline-prompt">{{ promptText(entry) }}</p>
                </template>
                <template v-else>
                  <span class="timeline-dot timeline-dot-request" />
                  <div class="request-card">
                    <div class="request-top">
                      <span class="request-title"><span class="request-time">{{ clockText(entry.at) }}</span>{{ t.sessions.request(entry.index) }}<template v-if="entry.is_subagent">{{ t.sessions.subagent }}</template></span>
                      <span class="request-marks">
                        <span v-if="entry.tag" class="advice-tag" :class="entry.tag.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ entry.tag.tag }}</span>
                        <strong v-if="deltaText(entry.weekly_delta)">{{ deltaText(entry.weekly_delta) }}</strong>
                      </span>
                    </div>
                    <p class="request-tokens">{{ tokenLine(detail.tool, entry.tokens) }}</p>
                  </div>
                </template>
              </li>
            </ol>
            <button v-if="hiddenRequests > 0" type="button" class="sessions-more" @click="requestLimit += 50">{{ t.common.showMore }}</button>
            <p class="session-footnote">{{ t.sessions.footnote }}</p>
          </div>
        </template>
      </aside></Transition>
  </div>
</template>
