<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { CircleAlert, CircleHelp, Cpu, Folder, GitFork, List, Zap } from 'lucide-vue-next'
import { getDeepSeekUsage, type CostRank, type DeepSeekPeriod, type DeepSeekUsage } from '../api'
import PageSkeleton from './PageSkeleton.vue'
import { vSelectionSlide } from './selectionSlide'
import { costSegments, segmentWidths, yuan } from '../deepseekLogic'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usageRevision } from '../realtime'
import { clock, compactText, intlLocale, m, messages, monthDay } from '../i18n/index.ts'

const router = useRouter()
const t = messages
const periods: DeepSeekPeriod[] = ['today', 'week', 'month']
const period = ref<DeepSeekPeriod>(pageSelection.usage.deepseekPeriod)
const key = (value: DeepSeekPeriod) => pageCacheKey('usageDetail', 'DeepSeek', value)
const usage = ref<DeepSeekUsage | null>(getPageCache<DeepSeekUsage>(key(period.value)) ?? null)
const pending = ref(usage.value === null)
const loading = ref(false)
const error = ref('')
const barsReady = ref(usage.value !== null)
const dimension = ref(pageSelection.usage.dimension)
let request = 0
let loadingTimer: ReturnType<typeof setTimeout> | undefined

watch(dimension, value => { pageSelection.usage.dimension = value })
const dimensions = computed(() => [
  { key: 'sessions', name: m().usage.dimSessions, icon: List },
  { key: 'projects', name: m().usage.dimProjects, icon: Folder },
  { key: 'models', name: m().usage.dimModels, icon: Cpu },
  { key: 'subagents', name: m().usage.dimSubagents, icon: GitFork },
] as const)

async function load(next: DeepSeekPeriod, quiet = false) {
  const ticket = ++request
  period.value = next
  pageSelection.usage.deepseekPeriod = next
  const cached = getPageCache<DeepSeekUsage>(key(next))
  if (cached) usage.value = cached
  if (!cached && !quiet) {
    pending.value = true
    barsReady.value = false
    loadingTimer = setTimeout(() => { if (ticket === request) loading.value = true }, 250)
  }
  error.value = ''
  const version = cacheVersion(key(next))
  try {
    const result = await getDeepSeekUsage(next)
    if (!cacheVersionMatches(key(next), version) || ticket !== request) return
    usage.value = result
    setPageCache(key(next), result)
  } catch (cause) {
    if (ticket === request && !cached) error.value = String(cause)
  } finally {
    if (ticket === request) {
      if (loadingTimer) clearTimeout(loadingTimer)
      pending.value = false
      loading.value = false
      await nextTick()
      requestAnimationFrame(() => requestAnimationFrame(() => { if (ticket === request) barsReady.value = true }))
    }
  }
}

onMounted(() => { void load(period.value, usage.value !== null) })
onUnmounted(() => { request++; if (loadingTimer) clearTimeout(loadingTimer) })
watch(usageRevision, () => { void load(period.value, true) })

/** The date range under each period button. */
function rangeText(value: DeepSeekPeriod) {
  const today = new Date()
  if (value === 'today') return monthDay(today.toISOString())
  if (value === 'week') {
    const start = new Date(today)
    start.setDate(start.getDate() - 6)
    return `${monthDay(start.toISOString())}–${monthDay(today.toISOString())}`
  }
  return new Intl.DateTimeFormat(intlLocale(), { month: 'short' }).format(today)
}
const balanceText = computed(() => {
  const overview = usage.value?.overview
  if (!overview?.balance) return ''
  const total = yuan(overview.balance.total)
  if (overview.days_left === null) return m().deepseek.balanceOnly(total)
  return overview.days_left < 1 ? m().deepseek.balanceUnderDay(total) : m().deepseek.balanceLine(total, Math.round(overview.days_left))
})
const segments = computed(() => usage.value ? costSegments(usage.value) : [])
const widths = computed(() => segmentWidths(segments.value))
const level = (kind: string, index: number) => kind === 'title' ? 5 : kind === 'other' ? 4 : Math.min(index, 3)
const rows = computed<CostRank[]>(() => usage.value?.[dimension.value] ?? [])
const visibleRows = computed(() => rows.value.slice(0, 8))
const maxCost = computed(() => Math.max(0, ...rows.value.map(row => row.cost)))
const rowSubtitle = (row: CostRank) => dimension.value === 'sessions'
  ? row.project_name
  : m().deepseek.requestsTokens(row.requests, compactText(row.tokens))
const topRequests = computed(() => usage.value?.high_requests.slice(0, 6) ?? [])
</script>

<template>
  <nav class="usage-window-list" :aria-label="t.usage.windows">
    <button v-for="item in periods" :key="item" :class="{ active: period === item }" :aria-current="period === item ? 'true' : undefined" @click="load(item)">
      <strong>{{ t.deepseek.period[item] }}</strong><span>{{ rangeText(item) }}</span>
    </button>
  </nav>
  <PageSkeleton v-if="pending && loading" kind="usageDetail" />
  <div v-else-if="pending" class="page-await" />
  <div v-else-if="error && !usage" class="center-state"><CircleAlert :size="28" /><strong>{{ t.usage.detailFailed }}</strong><p>{{ error }}</p><button @click="load(period)">{{ t.common.retry }}</button></div>
  <template v-else-if="usage">
    <section class="card usage-overview content-reveal" :aria-label="t.usage.overview">
      <div class="usage-overview-top">
        <div class="usage-overview-main"><span>{{ t.deepseek.spent[period] }}</span><strong>{{ yuan(usage.cost) }}</strong><span class="origin-pill pill-neutral">{{ t.deepseek.estimateLocal }}</span></div>
        <span v-if="balanceText" class="usage-reset">{{ balanceText }}</span>
      </div>
      <div class="usage-bar" role="img" :aria-label="`${t.deepseek.spent[period]} ${yuan(usage.cost)}`">
        <span v-for="(part, index) in segments" :key="part.key" :class="`usage-segment usage-level-${level(part.kind, index)}`" :style="{ width: barsReady ? `${widths[index]}%` : '0%' }" />
        <span v-if="!segments.length" class="usage-remainder" :style="{ flex: '1 1 auto' }" />
      </div>
      <div v-if="segments.length" class="usage-legend">
        <span v-for="(part, index) in segments" :key="part.key"><i :class="`usage-level-${level(part.kind, index)}`" />{{ part.label }} {{ yuan(part.cost) }}<span v-if="part.kind === 'title'" class="usage-help-toggle" :title="t.deepseek.titleHelp(usage.title_requests)"><CircleHelp :size="13" /></span></span>
      </div>
      <p class="usage-note">{{ t.deepseek.costNote }}</p>
      <p v-if="usage.unpriced_requests" class="usage-note">{{ t.deepseek.unpriced(usage.unpriced_requests) }}</p>
    </section>
    <div class="usage-grid content-reveal">
      <section class="card usage-ranking" :aria-label="t.usage.ranking"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><List :size="17" /></span><strong>{{ t.usage.ranking }}</strong></div><div v-selection-slide class="segmented"><button v-for="item in dimensions" :key="item.key" :class="{ active: dimension === item.key }" :aria-pressed="dimension === item.key" @click="dimension = item.key"><component :is="item.icon" :size="16" />{{ item.name }}</button></div></div>
        <template v-if="visibleRows.length"><div v-for="row in visibleRows" :key="row.key" class="usage-rank-row"><div class="usage-rank-name"><strong :title="row.name">{{ row.name }}</strong><small>{{ rowSubtitle(row) }}</small></div><div class="usage-rank-track"><span :style="{ width: barsReady ? `${maxCost ? Math.max(0, row.cost / maxCost * 100) : 0}%` : '0%' }" /></div><span class="usage-rank-tokens">{{ compactText(row.tokens) }}</span><strong class="usage-rank-delta">{{ yuan(row.cost) }}</strong></div><p v-if="rows.length > 8" class="usage-more">{{ t.usage.more(rows.length - 8) }}</p></template>
        <p v-else class="table-empty">{{ t.deepseek.noUsage }}</p>
      </section>
      <section class="card usage-prompts" :aria-label="t.deepseek.highRequests"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><Zap :size="17" /></span><strong>{{ t.deepseek.highRequests }}</strong></div><small>{{ t.deepseek.noContent }}</small></div>
        <template v-if="topRequests.length"><button v-for="item in topRequests" :key="`${item.session_id}-${item.at}`" class="usage-prompt-row" @click="router.push({ path: '/sessions', query: { tool: 'DeepSeek', id: item.session_id } })"><span class="usage-prompt-info"><small>{{ clock(item.at) }}</small><strong :title="item.title">{{ item.title }}</strong></span><strong class="usage-prompt-delta">+{{ yuan(item.cost) }}</strong></button></template>
        <p v-else class="table-empty">{{ t.deepseek.noRequests }}</p>
      </section>
    </div>
  </template>
</template>
