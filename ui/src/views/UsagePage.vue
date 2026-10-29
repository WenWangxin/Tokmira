<script setup lang="ts">
import { clock, compactText, messages, m, monthDay } from '../i18n/index.ts'
const t = messages
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Cpu, Folder, GitFork, List, Zap, Database, CircleAlert, CircleHelp } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import { getAdvice, getUsageDetail, getUsageWindows, promptTag, type AdviceReport, type Tool, type UsageDetail, type UsageRank, type UsageWindowRef } from '../api'
import PageSkeleton from '../components/PageSkeleton.vue'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usageRevision } from '../realtime'

const router = useRouter()
const windowsKey = pageCacheKey('usageWindows', 'all')
const cachedWindows = getPageCache<Record<Tool, UsageWindowRef[]>>(windowsKey)
const initialTool = pageSelection.usage.tool ?? (cachedWindows?.Claude.length ? 'Claude' : cachedWindows?.Codex.length ? 'Codex' : 'Claude')
const initialWindow = cachedWindows?.[initialTool].find(item => item.window_id === pageSelection.usage.windowIds[initialTool])
  ?? cachedWindows?.[initialTool][0] ?? null
const initialDetail = initialWindow ? getPageCache<UsageDetail>(pageCacheKey('usageDetail', initialWindow.tool, initialWindow.kind, initialWindow.window_id)) ?? null : null
const tool = ref<Tool>(initialTool)
const windows = ref<Record<Tool, UsageWindowRef[]>>(cachedWindows ?? { Claude: [], Codex: [] })
const selected = ref<UsageWindowRef | null>(initialWindow)
const detail = ref<UsageDetail | null>(initialDetail)
const dimension = ref<'sessions' | 'projects' | 'models' | 'subagents'>(pageSelection.usage.dimension)
const pending = ref(cachedWindows === undefined)
const loading = ref(false)
const detailPending = ref(Boolean(initialWindow && !initialDetail))
const detailLoading = ref(false)
const barsReady = ref(initialDetail !== null)
const reveal = ref(cachedWindows === undefined)
const error = ref('')
let loadingTimer: ReturnType<typeof setTimeout> | undefined
let detailTimer: ReturnType<typeof setTimeout> | undefined
let detailRequest = 0
const advice = ref<AdviceReport | null>(getPageCache<AdviceReport>(pageCacheKey('advice', 30)) ?? null)
watch([tool, dimension], () => {
  pageSelection.usage.tool = tool.value
  pageSelection.usage.dimension = dimension.value
})
const dimensions = computed(() => [
  { key: 'sessions', name: m().usage.dimSessions, icon: List },
  { key: 'projects', name: m().usage.dimProjects, icon: Folder },
  { key: 'models', name: m().usage.dimModels, icon: Cpu },
  { key: 'subagents', name: m().usage.dimSubagents, icon: GitFork },
] as const)

async function load(quiet = false) {
  if (!quiet && !cachedWindows) {
    pending.value = true
    loadingTimer = setTimeout(() => { loading.value = true }, 250)
  }
  error.value = ''
  const version = cacheVersion(windowsKey)
  try {
    const [claude, codex] = await Promise.all([getUsageWindows('Claude'), getUsageWindows('Codex')])
    if (!cacheVersionMatches(windowsKey, version)) return
    windows.value = { Claude: claude, Codex: codex }
    setPageCache(windowsKey, windows.value)
    const nextTool = windows.value[tool.value].length ? tool.value : claude.length ? 'Claude' : codex.length ? 'Codex' : 'Claude'
    tool.value = nextTool
    const nextWindow = windows.value[nextTool].find(item => item.window_id === pageSelection.usage.windowIds[nextTool])
      ?? windows.value[nextTool][0] ?? null
    if (nextWindow?.window_id !== selected.value?.window_id || nextWindow?.kind !== selected.value?.kind || nextWindow?.tool !== selected.value?.tool) {
      await selectWindow(nextWindow)
    } else if (nextWindow) {
      selected.value = nextWindow
      void selectWindow(nextWindow)
    }
  } catch (cause) {
    if (!quiet && !cachedWindows) error.value = String(cause)
  } finally {
    if (loadingTimer) clearTimeout(loadingTimer)
    pending.value = false
    loading.value = false
  }
}

async function selectWindow(window: UsageWindowRef | null) {
  const request = ++detailRequest
  if (detailTimer) clearTimeout(detailTimer)
  selected.value = window
  barsReady.value = false
  error.value = ''
  if (!window) { detail.value = null; detailPending.value = false; return }
  pageSelection.usage.windowIds[window.tool] = window.window_id
  const key = pageCacheKey('usageDetail', window.tool, window.kind, window.window_id)
  const cached = getPageCache<UsageDetail>(key)
  detail.value = cached ?? null
  detailPending.value = cached === undefined
  detailLoading.value = false
  barsReady.value = cached !== undefined
  if (cached === undefined) detailTimer = setTimeout(() => { if (request === detailRequest) detailLoading.value = true }, 250)
  const version = cacheVersion(key)
  try {
    const result = await getUsageDetail(window.tool, window.kind, window.window_id)
    if (!cacheVersionMatches(key, version)) return
    setPageCache(key, result)
    if (request === detailRequest) detail.value = result
  } catch (cause) {
    if (request === detailRequest && cached === undefined) error.value = String(cause)
  } finally {
    if (request === detailRequest) {
      if (detailTimer) clearTimeout(detailTimer)
      detailPending.value = false
      detailLoading.value = false
      if (cached === undefined && detail.value) {
        await nextTick()
        requestAnimationFrame(() => requestAnimationFrame(() => { if (request === detailRequest) barsReady.value = true }))
      } else barsReady.value = true
    }
  }
}

function selectTool(next: Tool) {
  if (next === tool.value || !windows.value[next].length) return
  tool.value = next
  const window = windows.value[next].find(item => item.window_id === pageSelection.usage.windowIds[next]) ?? windows.value[next][0] ?? null
  void selectWindow(window)
}

watch(usageRevision, () => { void load(true) })
onUnmounted(() => {
  detailRequest++
  if (loadingTimer) clearTimeout(loadingTimer)
  if (detailTimer) clearTimeout(detailTimer)
})
onMounted(() => {
  void load()
  // Tags are optional decoration; the page works without them.
  const key = pageCacheKey('advice', 30)
  const version = cacheVersion(key)
  getAdvice(30).then(report => { if (cacheVersionMatches(key, version)) { advice.value = report; setPageCache(key, report) } }).catch(() => {})
})
const tagFor = (sessionId: string, at: string) => promptTag(advice.value?.prompt_tags, tool.value, sessionId, at)
const rows = computed<UsageRank[]>(() => detail.value?.[dimension.value] ?? [])
const visibleRows = computed(() => rows.value.slice(0, 8))
const topPrompts = computed(() => detail.value?.high_prompts.slice(0, 6) ?? [])
const maxDelta = computed(() => Math.max(0, ...rows.value.map(row => row.delta)))
const toTime = (value: string) => clock(value)
const toDay = (value: string) => monthDay(value)
function windowLabel(window: UsageWindowRef) {
  if (window.kind === 'Weekly') return m().usage.windowWeekly
  if (window.is_current) return m().usage.windowCurrent
  const day = new Date(window.start).toDateString()
  if (day === new Date().toDateString()) return m().usage.today
  const yesterday = new Date(); yesterday.setDate(yesterday.getDate() - 1)
  return day === yesterday.toDateString() ? m().usage.yesterday : toDay(window.start)
}
const windowTimes = (window: UsageWindowRef) => window.kind === 'Weekly'
  ? `${toDay(window.start)}–${toDay(window.end)}` : `${toTime(window.start)}–${toTime(window.end)}`
const percent = (value: number) => `${value.toFixed(1)}%`
const tokens = (value: number) => compactText(value)
const originText = computed(() => {
  const origin = detail.value?.origin
  if (origin === 'Official') return tool.value === 'Claude' ? m().origin.claudeOfficial : m().origin.codexOfficial
  if (origin === 'Statusline') return m().origin.statusline
  if (origin === 'Log') return m().origin.log
  if (origin === 'Manual') return m().origin.manual
  return m().origin.estimated
})
const chartSegments = computed(() => {
  if (!detail.value) return []
  const sessions = detail.value.sessions
  const parts = sessions.slice(0, 4).map((item, index) => ({ name: item.name, value: item.delta, level: index }))
  const other = sessions.slice(4).reduce((sum, item) => sum + item.delta, 0)
  if (other > 0) parts.push({ name: m().usage.otherSessions, value: other, level: 4 })
  if (detail.value.unattributed_total > 0) parts.push({ name: m().usage.unattributed, value: detail.value.unattributed_total, level: 5 })
  return parts
})
const chartWidth = (value: number) => `${Math.max(0, Math.min(100, value))}%`
const breakdown = computed(() => detail.value?.official_weekly_breakdown?.rows.map(row =>
  `${row.display_name || row.key} ${percent(row.percent)}`).join(' · ') ?? '')
// Shares outside local Claude Code logs, quoted as-is from the official breakdown.
const offLogShare = computed(() => detail.value?.official_weekly_breakdown?.rows.filter(row => row.key !== 'claude_code').map(row =>
  `${row.display_name || row.key} ${percent(row.percent)}`).join(' · ') ?? '')
const helpOpen = ref(false)
const helpVisible = computed(() => helpOpen.value && (detail.value?.unattributed_total ?? 0) > 0)
</script>

<template>
  <div class="dashboard-page usage-page" :class="{ 'no-entry-reveal': !reveal }" :style="{ '--usage-tool': tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
    <header class="dashboard-header">
      <div><h1>{{ t.pages.usage.title }}</h1><p>{{ t.pages.usage.subtitle }}</p></div>
      <div v-selection-slide class="usage-tool-switch" :aria-label="t.usage.selectTool">
        <button v-for="item in (['Claude', 'Codex'] as const)" :key="item" :class="{ active: tool === item }" :disabled="!windows[item].length" :aria-pressed="tool === item" @click="selectTool(item)">
          <img v-if="item === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else />{{ item }}
        </button>
      </div>
    </header>
    <PageSkeleton v-if="pending && loading" kind="usage" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !selected" class="center-state"><CircleAlert :size="28" /><strong>{{ t.usage.loadFailed }}</strong><p>{{ error }}</p><button @click="load()">{{ t.common.retry }}</button></div>
    <div v-else-if="!windows[tool].length" class="center-state"><Database :size="28" /><strong>{{ t.usage.noData(tool) }}</strong><p>{{ t.usage.noDataBody }}</p><button @click="router.push({ path: '/settings', query: { section: 'sources' } })">{{ t.usage.goSettings }}</button></div>
    <template v-else>
      <nav class="usage-window-list" :aria-label="t.usage.windows">
        <button v-for="window in windows[tool]" :key="`${window.kind}-${window.window_id}`" :class="{ active: selected?.kind === window.kind && selected?.window_id === window.window_id }" :aria-current="selected?.window_id === window.window_id ? 'true' : undefined" @click="selectWindow(window)">
          <strong>{{ windowLabel(window) }}</strong><span>{{ windowTimes(window) }}</span>
        </button>
      </nav>
      <PageSkeleton v-if="detailPending && detailLoading" kind="usageDetail" />
      <div v-else-if="detailPending" class="page-await" />
      <div v-else-if="error && !detail" class="center-state"><CircleAlert :size="28" /><strong>{{ t.usage.detailFailed }}</strong><p>{{ error }}</p><button @click="selectWindow(selected)">{{ t.common.retry }}</button></div>
      <template v-else-if="detail">
        <section class="card usage-overview content-reveal" :aria-label="t.usage.overview">
          <div class="usage-overview-top"><div class="usage-overview-main"><span>{{ t.usage.windowUsed }}</span><strong>{{ percent(detail.used_percent) }}</strong><span class="origin-pill" :class="detail.origin === 'Manual' ? 'pill-warning' : detail.origin === 'Estimated' ? 'pill-neutral' : 'pill-success'">{{ originText }}</span></div><span class="usage-reset">{{ selected?.is_current ? t.usage.remaining(percent(detail.remaining_percent)) : t.usage.ended }} · {{ t.usage.resetsAt(toTime(detail.resets_at)) }}</span></div>
          <div class="usage-bar" role="img" :aria-label="t.usage.barLabel(percent(detail.used_percent), percent(detail.attributed_total), percent(detail.unattributed_total))"><span v-for="(part, index) in chartSegments" :key="`${part.name}-${index}`" :class="`usage-segment usage-level-${part.level}`" :style="{ width: barsReady ? chartWidth(part.value) : '0%' }" /><span class="usage-remainder" :style="{ width: barsReady ? chartWidth(detail.remaining_percent) : '0%' }" /></div>
          <div class="usage-legend"><span v-for="(part, index) in chartSegments" :key="`${part.name}-${index}`"><i :class="`usage-level-${part.level}`" />{{ part.name }} {{ percent(part.value) }}<button v-if="part.level === 5" type="button" class="usage-help-toggle" :class="{ active: helpVisible }" :title="t.usage.unattributedHelp.toggle" :aria-label="t.usage.unattributedHelp.toggle" :aria-expanded="helpVisible" aria-controls="usage-unattributed-help" @click="helpOpen = !helpOpen"><CircleHelp :size="13" /></button></span></div>
          <div v-if="helpVisible" id="usage-unattributed-help" class="usage-help" role="region" :aria-label="t.usage.unattributedHelp.title">
            <strong>{{ t.usage.unattributedHelp.title }}</strong>
            <h3>{{ t.usage.unattributedHelp.includesTitle }}</h3>
            <p class="settings-note">{{ tool === 'Claude' ? t.usage.unattributedHelp.includesClaude : t.usage.unattributedHelp.includesCodex }}<template v-if="tool === 'Claude' && offLogShare"> {{ t.usage.unattributedHelp.officialShare(offLogShare) }}</template></p>
            <h3>{{ t.usage.unattributedHelp.excludesTitle }}</h3>
            <p class="settings-note">{{ t.usage.unattributedHelp.excludes }}</p>
            <h3>{{ t.usage.unattributedHelp.accuracyTitle }}</h3>
            <ul class="settings-note">
              <li>{{ tool === 'Claude' ? t.usage.unattributedHelp.shrinkClaude : t.usage.unattributedHelp.shrinkCodex }}</li>
              <li>{{ tool === 'Claude' ? t.usage.unattributedHelp.denserClaude : t.usage.unattributedHelp.denserCodex }}</li>
              <li v-if="tool === 'Claude'">{{ t.usage.unattributedHelp.desktopOnly }}</li>
            </ul>
            <p class="settings-note usage-help-basis">{{ t.usage.unattributedHelp.basis(percent(detail.used_percent), percent(detail.attributed_total), percent(detail.unattributed_total)) }}</p>
          </div>
          <p v-if="breakdown" class="usage-breakdown">{{ t.usage.officialBreakdown(breakdown) }}</p>
          <p class="usage-note">{{ breakdown ? t.usage.noteWithBreakdown : t.usage.noteUnattributed }}</p>
        </section>
        <div class="usage-grid content-reveal">
          <section class="card usage-ranking" :aria-label="t.usage.ranking"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><List :size="17" /></span><strong>{{ t.usage.ranking }}</strong></div><div v-selection-slide class="segmented"><button v-for="item in dimensions" :key="item.key" :class="{ active: dimension === item.key }" :aria-pressed="dimension === item.key" @click="dimension = item.key"><component :is="item.icon" :size="16" />{{ item.name }}</button></div></div>
            <template v-if="visibleRows.length"><div v-for="row in visibleRows" :key="row.key" class="usage-rank-row"><div class="usage-rank-name"><strong :title="row.name">{{ row.name }}</strong><small>{{ row.subtitle }}</small></div><div class="usage-rank-track"><span :style="{ width: barsReady ? `${maxDelta ? Math.max(0, row.delta / maxDelta * 100) : 0}%` : '0%' }" /></div><span class="usage-rank-tokens">{{ tokens(row.tokens) }}</span><strong class="usage-rank-delta">{{ percent(row.delta) }}</strong></div><p v-if="rows.length > 8" class="usage-more">{{ t.usage.more(rows.length - 8) }}</p></template>
            <p v-else class="table-empty">{{ t.usage.noLocalRequests }}</p>
          </section>
          <section class="card usage-prompts" :aria-label="t.usage.highPrompts"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><Zap :size="17" /></span><strong>{{ t.usage.highPrompts }}</strong></div><small>{{ t.usage.noContent }}</small></div>
            <template v-if="topPrompts.length"><button v-for="prompt in topPrompts" :key="`${prompt.session_id}-${prompt.at}`" class="usage-prompt-row" @click="router.push({ path: '/sessions', query: { tool, id: prompt.session_id } })"><span class="usage-prompt-info"><small>{{ toTime(prompt.at) }}</small><strong :title="prompt.title">{{ prompt.title }}</strong><span v-if="tagFor(prompt.session_id, prompt.at)" class="advice-tag usage-prompt-tag" :class="tagFor(prompt.session_id, prompt.at)?.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ tagFor(prompt.session_id, prompt.at)?.tag }}</span></span><strong class="usage-prompt-delta">+{{ percent(prompt.delta) }}</strong></button></template>
            <p v-else class="table-empty">{{ detail.sessions.length ? t.usage.noPrompts : t.usage.noLocalRequests }}</p>
          </section>
        </div>
      </template>
    </template>
  </div>
</template>
