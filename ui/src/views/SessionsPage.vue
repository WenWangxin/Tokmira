<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { CircleAlert, Database, Search, X } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import PageSkeleton from '../components/PageSkeleton.vue'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usePageRequest } from '../usePageRequest'
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

async function load() {
  await listRequest.load(listKey, () => getSessions(7))
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
  return names.length ? `暂无 ${names.join('、')} 额度数据，额度列无法换算` : ''
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
function closeSession() {
  router.replace({ path: '/sessions' })
}
function selected(row: SessionRow) {
  return routeSession.value?.tool === row.tool && routeSession.value.id === row.session_id
}
const sourceLabel = (source: Source) => ({
  ClaudeCli: 'Claude Code · 命令行', ClaudeDesktop: 'Claude · 桌面版',
  CodexCli: 'Codex · 命令行', CodexDesktop: 'Codex · 桌面版', Unknown: '未知来源',
})[source]
const sourcePhrase = (source: Source) => ({
  ClaudeCli: 'Claude Code 命令行', ClaudeDesktop: 'Claude 桌面版',
  CodexCli: 'Codex 命令行', CodexDesktop: 'Codex 桌面版', Unknown: '未知来源',
})[source]
const tokens = (value: number) => value >= 100_000_000
  ? `${Number((value / 100_000_000).toFixed(1))} 亿`
  : value >= 10_000_000 ? `${Math.round(value / 10_000)} 万`
  : value >= 10_000 ? `${Number((value / 10_000).toFixed(1))} 万` : value.toLocaleString('zh-CN')
const hitText = (value: number | null) => value === null ? '—' : `${(value * 100).toFixed(0)}%`
const quotaText = (value: number, available: boolean) => available ? `${value.toFixed(1)}%` : '—'
const grouped = (value: number) => value.toLocaleString('en-US')
function tokenLine(tool: Tool, item: TokenCounts) {
  const read = grouped(item.cache_read)
  const output = grouped(item.output)
  if (tool === 'Codex') return `输入 ${grouped(item.input)} · 读缓存 ${read} · 输出 ${output}`
  const write = item.cache_write_5m + item.cache_write_1h + item.cache_write_unspecified
  return `写缓存 ${grouped(write)} · 读缓存 ${read} · 输出 ${output}`
}
function clockText(value: string) {
  return new Date(value).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
}
function promptText(entry: Extract<TimelineEntry, { kind: 'Prompt' }>) {
  const gap = entry.gap_minutes_from_previous_prompt
  const text = `${clockText(entry.at)} 提交提示`
  return gap !== null && gap >= 30 ? `${text} · 距上次 ${gap} 分钟` : text
}
function deltaText(value: number) {
  if (Math.abs(value).toFixed(1) === '0.0') return ''
  return value > 0 ? `+${value.toFixed(1)}%` : `${value.toFixed(1)}%`
}
const visibleEntries = computed(() => {
  if (!detail.value) return []
  let requests = 0
  const shown: TimelineEntry[] = []
  for (const entry of detail.value.entries) {
    if (entry.kind === 'Request') {
      if (requests >= requestLimit.value) break
      requests += 1
    }
    shown.push(entry)
  }
  return shown
})
const hiddenRequests = computed(() => {
  const total = detail.value?.entries.filter(entry => entry.kind === 'Request').length ?? 0
  return Math.max(0, total - requestLimit.value)
})
const listedSession = computed(() => {
  const selected = routeSession.value
  if (!selected) return null
  return rows.value.find(row => row.tool === selected.tool && row.session_id === selected.id) ?? null
})
const detailTitle = computed(() => detail.value?.title || detail.value?.display_name || listedSession.value?.title || listedSession.value?.display_name || routeSession.value?.id || '会话')
const detailSubtitle = computed(() => {
  const source = detail.value ?? listedSession.value
  if (!source) return ''
  const project = source.project_name || '未知项目'
  return `${project} · ${sourcePhrase(source.source)} · ${source.model || '—'}`
})
</script>

<template>
  <div class="dashboard-page sessions-page" :class="{ 'no-entry-reveal': !reveal }">
    <header class="dashboard-header">
      <div>
        <h1>会话</h1>
        <p>近 7 天 · 按额度消耗排序 · 点击查看请求明细</p>
      </div>
      <div class="sessions-toolbar">
        <label class="session-search">
          <Search :size="16" aria-hidden="true" />
          <input v-model="query" type="search" placeholder="搜索会话或项目" aria-label="搜索会话或项目" />
        </label>
        <div v-selection-slide class="segmented" role="group" aria-label="筛选来源">
          <button type="button" :class="{ active: toolFilter === 'all' }" :aria-pressed="toolFilter === 'all'" @click="toolFilter = 'all'">全部</button>
          <button v-for="tool in (['Claude', 'Codex'] as const)" :key="tool" type="button" :class="{ active: toolFilter === tool }" :aria-pressed="toolFilter === tool" @click="toolFilter = tool">{{ tool }}</button>
        </div>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="sessions" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !listRequest.data.value" class="center-state"><CircleAlert :size="28" /><strong>读取会话失败</strong><p>{{ error }}</p><button type="button" @click="load">重试</button></div>
    <div v-else-if="!rows.length" class="center-state"><Database :size="28" /><strong>近 7 天没有会话</strong><p>开始使用 Claude Code 或 Codex 后，会话会显示在这里。</p></div>
    <div v-else-if="!filtered.length" class="center-state"><Search :size="28" /><strong>没有匹配的会话</strong><p>换一个关键词，或切换来源后再试。</p></div>
    <section v-else class="card sessions-table content-reveal" aria-label="会话列表">
      <div class="session-head"><span>来源</span><span>会话</span><span>请求</span><span>token</span><span>缓存命中</span><span>额度</span></div>
      <button v-for="row in visibleRows" :key="`${row.tool}-${row.session_id}`" type="button" class="session-line" :class="{ active: selected(row) }" :aria-current="selected(row) ? 'true' : undefined" @click="openSession(row)">
        <span class="row-source"><span class="row-icon"><img v-if="row.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span>{{ sourceLabel(row.source) }}</span>
        <span class="session-name"><strong :title="row.title || row.display_name">{{ row.title || row.display_name }}</strong><small v-if="row.title && row.project_name">{{ row.project_name }}</small></span>
        <span class="session-metric">{{ row.requests.toLocaleString('zh-CN') }}</span>
        <span class="session-metric">{{ tokens(row.total_tokens) }}</span>
        <span class="session-metric" :class="{ 'error-text': row.cache_hit_rate !== null && row.cache_hit_rate < 0.5 }">{{ hitText(row.cache_hit_rate) }}</span>
        <span class="session-metric session-quota">{{ quotaText(row.weekly_delta, row.has_quota_data) }}</span>
      </button>
      <button v-if="filtered.length > visibleCount" type="button" class="sessions-more" @click="visibleCount += 50">显示更多</button>
      <p v-if="quotaNote" class="sessions-quota-note">{{ quotaNote }}</p>
    </section>

    <Transition name="drawer-mask"><button v-if="routeSession" type="button" class="session-mask" aria-label="关闭会话详情" @click="closeSession" /></Transition>
    <Transition name="drawer"><aside v-if="routeSession" class="session-drawer" role="dialog" aria-modal="true" aria-labelledby="session-detail-title" :style="{ '--session-tool': (detail?.tool ?? routeSession.tool) === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
        <div class="session-drawer-head">
          <div>
            <h2 id="session-detail-title">{{ detailTitle }}</h2>
            <p v-if="detailSubtitle">{{ detailSubtitle }}</p>
          </div>
          <button type="button" class="session-close" aria-label="关闭" @click="closeSession"><X :size="18" /></button>
        </div>
        <PageSkeleton v-if="detailPending && detailLoading" kind="sessionDetail" />
        <div v-else-if="detailPending" class="session-drawer-status" />
        <div v-else-if="detailError" class="session-drawer-status"><CircleAlert :size="24" /><strong>读取请求明细失败</strong><p>{{ detailError }}</p></div>
        <template v-else-if="detail">
          <div class="session-stats">
            <div class="session-stat"><small>请求</small><strong>{{ detail.requests.toLocaleString('zh-CN') }}</strong></div>
            <div class="session-stat"><small>token</small><strong>{{ tokens(detail.total_tokens) }}</strong></div>
            <div class="session-stat"><small>缓存命中</small><strong :class="{ 'error-text': detail.cache_hit_rate !== null && detail.cache_hit_rate < 0.5 }">{{ hitText(detail.cache_hit_rate) }}</strong></div>
            <div class="session-stat"><small>额度</small><strong class="session-stat-quota">{{ quotaText(detail.weekly_delta, detail.has_quota_data) }}</strong></div>
          </div>
          <div class="session-scroll">
            <h3>请求明细</h3>
            <ol class="timeline">
              <li v-for="(entry, index) in visibleEntries" :key="`${entry.kind}-${entry.at}-${index}`" class="timeline-item">
                <template v-if="entry.kind === 'Prompt'">
                  <span class="timeline-dot timeline-dot-prompt" />
                  <p class="timeline-prompt">{{ promptText(entry) }}</p>
                </template>
                <template v-else>
                  <span class="timeline-dot timeline-dot-request" />
                  <div class="request-card">
                    <div class="request-top">
                      <span class="request-title"><span class="request-time">{{ clockText(entry.at) }}</span>请求 #{{ entry.index }}<template v-if="entry.is_subagent"> · 子代理</template></span>
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
            <button v-if="hiddenRequests > 0" type="button" class="sessions-more" @click="requestLimit += 50">显示更多</button>
            <p class="session-footnote">提示内容不会被记录，仅保留时间。</p>
          </div>
        </template>
      </aside></Transition>
  </div>
</template>
