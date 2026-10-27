<script setup lang="ts">
import { showToast } from '../toast'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Share, RefreshCw, CircleCheck, CircleAlert, Coins, Database, TrendingUp, ShieldCheck, Lightbulb, ChevronRight } from 'lucide-vue-next'
import { getAdvice, getOverview, getSettings, refreshQuotas, rescan, type AdviceItem, type AdviceReport, type Overview, type Origin, type QuotaDisplay, type Tool, type Source } from '../api'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import Sparkline from '../components/Sparkline.vue'
import QuotaTrend from '../components/QuotaTrend.vue'
import { remainingQuotaColor } from '../components/quotaChart'
import ShareDialog from '../components/ShareDialog.vue'
import AnimatedNumber from '../components/AnimatedNumber.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import { dashboardRefreshToken, pendingDashboardNotice } from '../onboardingState'
import { claudeNeedsAuthorization, lastQuotaResult, usageRevision } from '../realtime'
import SourceStatusPanel from '../components/SourceStatusPanel.vue'
import { overallTone, sourceRows, timeAgo } from '../sourceStatus'
import { cacheVersion, cacheVersionMatches, getPageCache, invalidatePageCache, pageCacheKey, setPageCache } from '../pageCache'
import { clock, compactNumber, compactText, formatInteger, intlLocale, listJoin, longMonthDay, m, messages, monthDay } from '../i18n/index.ts'
import { quotaMessage } from '../settingsLogic'

const router = useRouter()
const t = messages
const overviewKey = pageCacheKey('overview')
const initialOverview = getPageCache<Overview>(overviewKey) ?? null
const overview = ref<Overview | null>(initialOverview)
const loading = ref(false)
const pending = ref(initialOverview === null)
let loadingTimer: ReturnType<typeof setTimeout> | undefined
let loadTicket = 0
const reveal = ref(initialOverview === null)
const refreshing = ref(false)
const error = ref('')
const refreshError = ref('')
const selectedTool = ref<Tool>(initialOverview?.tools.filter(item => item.five_hour.used_percent !== null)
  .sort((a, b) => (b.five_hour.used_percent ?? 0) - (a.five_hour.used_percent ?? 0))[0]?.tool ?? 'Claude')
let userSelectedTool = false
const selectedRange = ref<'five_hour' | 'today' | 'week'>('five_hour')
const selectedList = ref<'sessions' | 'prompts'>('sessions')
const advice = ref<AdviceReport | null>(getPageCache<AdviceReport>(pageCacheKey('advice', 7)) ?? null)
const shareOpen = ref(false)
const ringReady = ref(initialOverview !== null)
const firstContent = ref(initialOverview === null)

// Advice is secondary: a failure here must not block the dashboard.
async function loadAdvice() {
  const key = pageCacheKey('advice', 7)
  const version = cacheVersion(key)
  try {
    const result = await getAdvice(7)
    if (cacheVersionMatches(key, version)) { advice.value = result; setPageCache(key, result) }
  } catch { /* Keep the last successful diagnosis. */ }
}

async function load(quiet = false) {
  const ticket = ++loadTicket
  // Live updates must not flip the trend chart between tools; only the first data picks one.
  const hadData = overview.value !== null
  if (!quiet && !overview.value) {
    pending.value = true
    loadingTimer = setTimeout(() => { loading.value = true }, 250)
  }
  error.value = ''
  const version = cacheVersion(overviewKey)
  try {
    const data = await getOverview()
    if (!cacheVersionMatches(overviewKey, version) || ticket !== loadTicket) return
    overview.value = data
    setPageCache(overviewKey, data)
    void loadAdvice()
    const available = data.tools.filter(item => item.five_hour.used_percent !== null)
    if ((!userSelectedTool && !hadData) || !available.some(item => item.tool === selectedTool.value)) {
      selectedTool.value = available.sort((a, b) => (b.five_hour.used_percent ?? 0) - (a.five_hour.used_percent ?? 0))[0]?.tool ?? 'Claude'
    }
  } catch (cause) {
    if (ticket === loadTicket && !overview.value) error.value = String(cause)
  } finally {
    if (ticket !== loadTicket) return
    if (loadingTimer) clearTimeout(loadingTimer)
    loadingTimer = undefined
    pending.value = false
    loading.value = false
    if (overview.value && firstContent.value) {
      await nextTick()
      requestAnimationFrame(() => requestAnimationFrame(() => { ringReady.value = true }))
      const styles = getComputedStyle(document.documentElement)
      const staggerTime = Number.parseFloat(styles.getPropertyValue('--motion-medium')) + Number.parseFloat(styles.getPropertyValue('--motion-stagger-5'))
      window.setTimeout(() => { firstContent.value = false }, staggerTime)
    }
  }
}
async function refresh() {
  if (refreshing.value) return
  refreshing.value = true
  const spinStart = performance.now()
  error.value = ''
  refreshError.value = ''
  try {
    await rescan()
    invalidatePageCache('overview', 'advice', 'usageWindows', 'usageDetail', 'sessions', 'sessionDetail')
    const result = await refreshQuotas()
    refreshError.value = [result.claude, result.codex].filter(outcome => outcome.state === 'error' || outcome.state === 'backoff' || outcome.state === 'needs_authorization').map(quotaMessage).join(m().onboarding.failureSeparator)
    if (refreshError.value) showToast(refreshError.value, 'warning')
    overview.value = await getOverview()
    setPageCache(overviewKey, overview.value)
    void loadAdvice()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const spinTime = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--motion-spin'))
      await new Promise(resolve => setTimeout(resolve, (spinTime - (performance.now() - spinStart) % spinTime) % spinTime))
    }
    refreshing.value = false
  }
}
// Source status panel behind the round status button in the header.
const statusOpen = ref(false)
const statusAnchor = ref<HTMLElement | null>(null)
const sourceSettings = ref<{ claude_official_query: boolean; codex_app_server: boolean } | null>(null)
const now = ref(Date.now())
let nowTimer: ReturnType<typeof setInterval> | undefined
async function loadSourceSettings() {
  try { sourceSettings.value = await getSettings() } catch { /* The panel shows sources as off until settings load. */ }
}
const statusRows = computed(() => overview.value ? sourceRows({
  claudeOfficialEnabled: Boolean(sourceSettings.value?.claude_official_query),
  codexOfficialEnabled: Boolean(sourceSettings.value?.codex_app_server),
  recorder: overview.value.recorder,
  times: overview.value.quota_sources ?? null,
  claudeNeedsAuthorization: claudeNeedsAuthorization.value,
  lastResult: lastQuotaResult.value,
  now: now.value,
}) : [])
const statusTone = computed(() => overallTone(statusRows.value, error.value))
const logsUpdated = computed(() => overview.value ? m().time.updated(timeAgo(overview.value.generated_at, now.value)) : null)
function toggleStatus() {
  statusOpen.value = !statusOpen.value
  if (statusOpen.value) { now.value = Date.now(); void loadSourceSettings() }
}
function onStatusPointer(event: PointerEvent) {
  if (statusOpen.value && !statusAnchor.value?.contains(event.target as Node)) statusOpen.value = false
}
function onStatusKey(event: KeyboardEvent) {
  if (statusOpen.value && event.key === 'Escape') statusOpen.value = false
}
function openSourceSettings() {
  statusOpen.value = false
  void router.push('/settings')
}
onMounted(() => {
  void loadSourceSettings()
  document.addEventListener('pointerdown', onStatusPointer)
  document.addEventListener('keydown', onStatusKey)
  nowTimer = setInterval(() => { now.value = Date.now() }, 30_000)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', onStatusPointer)
  document.removeEventListener('keydown', onStatusKey)
  if (nowTimer) clearInterval(nowTimer)
})

/** Asks for the keychain once, by hand, so background queries can read it afterwards. */
async function authorizeClaude() {
  showToast(m().dashboard.keychainHint, 'info')
  await refresh()
  if (!claudeNeedsAuthorization.value) showToast(m().dashboard.claudeUpdated, 'success')
}
onMounted(() => { void load() })
onUnmounted(() => { loadTicket++; if (loadingTimer) clearTimeout(loadingTimer) })
watch(usageRevision, () => { if (!refreshing.value) void load(true) })
watch(dashboardRefreshToken, () => {
  const pending = pendingDashboardNotice.value
  if (pending) {
    showToast(pending.text, pending.tone)
    pendingDashboardNotice.value = null
  }
  invalidatePageCache('overview', 'advice', 'usageWindows', 'usageDetail', 'sessions', 'sessionDetail')
  void load(true)
})
const adviceRank = (item: AdviceItem) => [item.avoidable_weekly_pct ?? -1, item.avoidable_cost_usd, item.overhead_cost_usd]
const topAdvice = computed(() => (advice.value?.tools.flatMap(tool => tool.items) ?? [])
  .slice().sort((a, b) => {
    const [x, y] = [adviceRank(a), adviceRank(b)]
    return y[0] - x[0] || y[1] - x[1] || y[2] - x[2]
  }).slice(0, 3))
const adviceCount = computed(() => advice.value?.tools.reduce((sum, tool) => sum + tool.items.length, 0) ?? 0)
/** Savings headline from the tool with the largest avoidable share; quota pools are never summed. */
const adviceHeadline = computed(() => {
  const tools = advice.value?.tools.filter(tool => tool.items.length) ?? []
  const withPct = tools.filter(tool => tool.summary.avoidable_weekly_pct !== null)
    .sort((a, b) => (b.summary.avoidable_weekly_pct ?? 0) - (a.summary.avoidable_weekly_pct ?? 0))[0]
  if (withPct) return { value: `${(withPct.summary.avoidable_weekly_pct ?? 0).toFixed(1)}%`, unit: m().dashboard.weeklyOf(withPct.tool) }
  const byCost = tools.sort((a, b) => b.summary.avoidable_cost_usd - a.summary.avoidable_cost_usd)[0]
  return byCost ? { value: `$${byCost.summary.avoidable_cost_usd.toFixed(2)}`, unit: m().common.apiEquivalent } : null
})
const adviceImpact = (item: AdviceItem) => item.consumed_weekly_pct !== null
  ? `${item.consumed_weekly_pct.toFixed(1)}%` : `$${item.overhead_cost_usd.toFixed(2)}`
const empty = computed(() => overview.value?.summary.sessions === 0 && overview.value?.summary.requests === 0)
const toolsWithData = computed(() => overview.value?.tools.filter(item => item.five_hour.used_percent !== null) ?? [])
const trendTools = computed(() => toolsWithData.value.map(item => item.tool))
const tableRows = computed(() => selectedList.value === 'sessions'
  ? overview.value?.recent_sessions.slice(0, 5) ?? []
  : overview.value?.recent_high_prompts.slice(0, 5) ?? [])
const time = (value: string | null, options?: Intl.DateTimeFormatOptions) => value ? new Date(value).toLocaleString(intlLocale(), options ?? { hour: '2-digit', minute: '2-digit', hour12: false }) : '—'
const updatedAt = (value: string) => `${longMonthDay(value)} ${clock(value)}`
const dateSlash = (value: string) => monthDay(value)
const percent = (value: number | null, digits = 0) => value === null ? '—' : `${(value * 100).toFixed(digits)}%`
const quotaPercent = (value: number | null) => value === null ? '—' : Number(value.toFixed(1)).toString()
const resetRemaining = (value: string | null) => {
  if (!value) return '—'
  const minutes = Math.max(0, Math.ceil((Date.parse(value) - Date.now()) / 60_000))
  return m().dashboard.resetIn(Math.floor(minutes / 60), minutes % 60)
}
const hitTime = (value: string) => {
  const minutes = Math.max(0, Math.ceil((Date.parse(value) - Date.now()) / 60_000))
  return m().dashboard.hitIn(Math.floor(minutes / 60), minutes % 60)
}
const forecast = (quota: QuotaDisplay) => {
  if (quota.window_passed) return { text: m().dashboard.forecastWaiting, sub: m().dashboard.forecastWaitingSub, tone: '' }
  if (quota.used_percent === null) return { text: '—', sub: '—', tone: '' }
  if (quota.forecast === 'Ample') return { text: m().dashboard.ample, sub: m().dashboard.ampleSub, tone: 'success-text' }
  if (typeof quota.forecast === 'object') return { text: hitTime(quota.forecast.WillHit.at), sub: m().dashboard.hitSub, tone: 'warning-text' }
  return { text: m().dashboard.insufficient, sub: m().dashboard.noForecast, tone: '' }
}
const sourceText = (tool: Tool, origin: Origin | null) => {
  if (origin === 'Official') return tool === 'Claude' ? m().origin.claudeOfficial : m().origin.codexOfficial
  if (origin === 'Statusline') return m().origin.statusline
  if (origin === 'Log') return m().origin.log
  if (origin === 'Manual') return m().origin.manual
  return m().origin.none
}
const planName = (tool: Tool, plan: string | null) => {
  if (!plan) return tool
  const known: Record<string, string> = tool === 'Claude'
    ? { pro: 'Claude Pro', max: 'Claude Max 5x', max_5x: 'Claude Max 5x', max_20x: 'Claude Max 20x' }
    : { plus: 'Codex · ChatGPT Plus', pro: 'Codex · ChatGPT Pro' }
  return known[plan.toLowerCase()] ?? tool
}
const compact = (value: number) => compactNumber(value, 1_000_000)
const todayNumber = computed(() => compact(overview.value?.today_token_total ?? 0))
const costParts = computed(() => (overview.value?.month_estimated_cost_usd ?? 0).toFixed(2).split('.'))
const valueText = computed(() => {
  const data = overview.value
  if (!data || data.subscription_total_usd == null) return m().dashboard.planUnknown
  if (!data.month_cost_by_tool) return m().dashboard.regenerate
  const unknown = data.tools.filter(item => item.subscription_usd == null && Object.hasOwn(data.month_cost_by_tool, item.tool))
  const note = unknown.length ? m().dashboard.unknownPlans(listJoin(unknown.map(item => item.tool))) : ''
  return m().dashboard.subscriptionValue(String(data.subscription_total_usd), data.subscription_value_multiple?.toFixed(1) ?? '—', note)
})
const cacheHitChangeText = computed(() => {
  const change = overview.value?.cache_hit_change_from_yesterday
  if (change == null) return m().dashboard.noComparison
  if (Math.abs(change) < 0.005) return m().dashboard.flat
  return change > 0 ? m().dashboard.changeUp(`${Math.abs(change * 100).toFixed(0)}%`) : m().dashboard.changeDown(`${Math.abs(change * 100).toFixed(0)}%`)
})
const recorderCount = computed(() => Number(overview.value?.recorder.statusline_installed) + Number(overview.value?.recorder.hooks_installed))
const recorderInfo = computed(() => {
  const recorder = overview.value?.recorder
  if (!recorder) return ''
  if (recorder.statusline_modified || recorder.hooks_modified) return m().dashboard.recorderModified
  if (!recorder.statusline_installed && !recorder.hooks_installed) return m().dashboard.recorderNone
  if (!recorder.statusline_installed) return m().dashboard.statuslineMissing
  if (!recorder.hooks_installed) return m().dashboard.hooksMissing
  return m().dashboard.recorderAll
})
const sourceLabel = (source: Source) => ({ ClaudeCli: m().sources.label.ClaudeCli, ClaudeDesktop: m().sources.label.ClaudeDesktop, CodexCli: m().sources.label.CodexCli, CodexDesktop: m().sources.label.CodexDesktop, Unknown: m().sources.label.Unknown })[source]
const rowTitle = (row: { title: string | null; project: string | null; session_id: string }) => row.title || row.project?.split(/[\\/]/).filter(Boolean).at(-1) || row.session_id.slice(0, 8)
const quotaDelta = (value: number) => value > 0 && value < .1 ? '<0.1%' : `${value.toFixed(1)}%`
const modelLabel = (model: string) => model || '—'
const selectTrendTool = (tool: Tool) => { selectedTool.value = tool; userSelectedTool = true }
const openSession = (row: { tool: Tool; session_id: string }) => {
  router.push({ path: '/sessions', query: { tool: row.tool, id: row.session_id } })
}
</script>

<template>
  <div class="dashboard-page" :class="{ 'no-entry-reveal': !reveal }">
    <header class="dashboard-header">
      <div><h1>{{ t.pages.dashboard.title }}</h1><p v-if="overview">{{ t.dashboard.headerUpdated(updatedAt(overview.generated_at), overview.summary.sessions) }}</p><p v-else>{{ t.dashboard.headerFallback }}</p></div>
      <div class="header-actions">
        <button class="round-action" :title="t.dashboard.share" :aria-label="t.dashboard.share" @click="shareOpen = true"><Share :size="21" /></button>
        <button class="round-action" :title="t.dashboard.refresh" :aria-label="t.dashboard.refresh" :disabled="refreshing || loading" @click="refresh"><RefreshCw :size="21" :class="{ spinning: refreshing }" /></button>
        <div ref="statusAnchor" class="state-anchor">
          <button class="round-action state-action" :class="statusTone === 'warning' ? 'state-warning' : 'state-success'" :title="statusTone === 'warning' ? t.dashboard.sourcesWarning : t.dashboard.sourcesOk" :aria-label="t.dashboard.sources" :aria-expanded="statusOpen" @click="toggleStatus">
            <CircleAlert v-if="statusTone === 'warning'" :size="22" /><CircleCheck v-else :size="22" />
          </button>
          <Transition name="popover"><SourceStatusPanel v-if="statusOpen" :rows="statusRows" :logs-updated="logsUpdated" :load-error="error" :busy="refreshing" @authorize="statusOpen = false; authorizeClaude()" @settings="openSourceSettings" /></Transition>
        </div>
      </div>
    </header>
    <div v-if="pending && loading" class="skeleton-layout" :aria-label="t.common.loading"><div class="skeleton" v-for="i in 9" :key="i" /></div>
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !overview" class="center-state"><CircleAlert :size="30" /><strong>{{ t.dashboard.loadFailed }}</strong><p>{{ error }}</p><button @click="load()">{{ t.common.retry }}</button></div>
    <div v-else-if="empty" class="center-state"><Database :size="32" /><strong>{{ t.dashboard.emptyTitle }}</strong><p>{{ t.dashboard.emptyBody }}</p><button @click="refresh">{{ t.dashboard.rescan }}</button></div>
    <div v-else-if="overview" class="dashboard-content content-reveal">
      <section class="quota-grid" :aria-label="t.dashboard.quotaSection">
        <article v-for="item in overview.tools" :key="item.tool" class="card quota-card" :class="{ 'first-card': firstContent }">
          <div class="quota-body">
            <div class="quota-top"><span class="tool-icon"><img v-if="item.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span><span class="plan-label">{{ planName(item.tool, item.plan) }}</span><span class="origin-pill" :class="item.five_hour.origin === 'Manual' ? 'pill-warning' : item.five_hour.used_percent === null || item.five_hour.origin === 'Estimated' ? 'pill-neutral' : 'pill-success'">{{ sourceText(item.tool, item.five_hour.origin) }}</span></div>
            <div class="quota-main"><div><div class="eyebrow">{{ t.dashboard.fiveHourUsed }}</div><div class="quota-number" :class="{ 'quota-passed': item.five_hour.window_passed }"><AnimatedNumber :initial-animation="reveal" :value="item.five_hour.window_passed ? t.dashboard.reset : quotaPercent(item.five_hour.used_percent)" /><span v-if="!item.five_hour.window_passed && item.five_hour.used_percent !== null">%</span></div><button v-if="item.tool === 'Claude' && claudeNeedsAuthorization" class="settings-link" :disabled="refreshing" @click="authorizeClaude">{{ t.dashboard.authorize }} <ChevronRight :size="14" /></button><button v-else-if="item.five_hour.used_percent === null && !item.five_hour.window_passed" class="settings-link" @click="router.push('/settings')">{{ t.dashboard.enableSource }} <ChevronRight :size="14" /></button></div>
              <div class="quota-ring"><svg viewBox="0 0 96 96" aria-hidden="true"><circle cx="48" cy="48" r="42" fill="none" stroke="var(--surface-container-high)" stroke-width="10"/><circle v-if="item.five_hour.used_percent !== null && !item.five_hour.window_passed" cx="48" cy="48" r="42" fill="none" :style="{ stroke: remainingQuotaColor(100 - item.five_hour.used_percent) }" stroke-width="10" stroke-dasharray="264 264" :stroke-dashoffset="ringReady ? 264 - Math.max(0, Math.min(100, 100 - item.five_hour.used_percent)) * 2.639 : 264" transform="rotate(-90 48 48)" /></svg><div class="ring-label"><strong>{{ item.five_hour.used_percent === null ? '—' : `${Math.max(0, 100 - item.five_hour.used_percent).toFixed(0)}%` }}</strong><span>{{ t.dashboard.remaining }}</span></div></div>
            </div>
            <div class="quota-facts"><div><span>{{ t.dashboard.nextReset }}</span><template v-if="item.five_hour.used_percent === 0 && !item.five_hour.resets_at"><strong>{{ t.dashboard.notStarted }}</strong><small>{{ t.dashboard.startsOnFirstUse }}</small></template><template v-else><strong>{{ time(item.five_hour.resets_at) }}</strong><small>{{ resetRemaining(item.five_hour.resets_at) }}</small></template></div><div><span>{{ t.dashboard.weekly }}</span><strong>{{ item.weekly.used_percent === null ? '—' : `${quotaPercent(item.weekly.used_percent)}%` }}</strong><small>{{ item.weekly.resets_at ? t.dashboard.resetsOn(dateSlash(item.weekly.resets_at)) : '—' }}</small></div><div><span>{{ t.dashboard.forecast }}</span><strong :class="forecast(item.five_hour).tone" class="forecast-text">{{ forecast(item.five_hour).text }}</strong><small>{{ forecast(item.five_hour).sub }}</small></div></div>
          </div>
        </article>
      </section>
      <section class="metric-grid" :aria-label="t.dashboard.metrics">
        <article class="card metric-card" :class="{ 'first-card': firstContent }"><div class="metric-head"><span class="metric-icon"><Coins :size="17" /></span><span>{{ t.dashboard.todayUsage }}</span><small>{{ t.dashboard.peak(overview.peak_hour === null ? '—' : `${String(overview.peak_hour).padStart(2, '0')}:00`) }}</small></div><div class="metric-value"><AnimatedNumber :initial-animation="reveal" :value="todayNumber.value" /><span>{{ todayNumber.unit }}</span></div><p>{{ overview.today_cache_read_share === null ? t.dashboard.noCacheRead : t.dashboard.fromCacheRead(percent(overview.today_cache_read_share)) }}</p><Sparkline :values="overview.hourly_tokens" /></article>
        <article class="card metric-card" :class="{ 'first-card': firstContent }"><div class="metric-head"><span class="metric-icon"><Database :size="17" /></span><span>{{ t.dashboard.cacheHitRate }}</span><small>{{ t.dashboard.today }}</small></div><div class="metric-value"><AnimatedNumber :initial-animation="reveal" :value="overview.today_cache_hit_rate === null ? '—' : (overview.today_cache_hit_rate * 100).toFixed(0)" /><span v-if="overview.today_cache_hit_rate !== null">%</span></div><p>{{ cacheHitChangeText }}</p><Sparkline :values="overview.daily_cache_hit_rate.map(day => day.rate)" /></article>
        <article class="card metric-card" :class="{ 'first-card': firstContent }"><div class="metric-head"><span class="metric-icon"><TrendingUp :size="17" /></span><span>{{ t.dashboard.monthCost }}</span><small>{{ t.dashboard.estimate }}</small></div><div class="metric-value">$<AnimatedNumber :initial-animation="reveal" :value="`${costParts[0]}.${costParts[1]}`" small-decimal /></div><p>{{ valueText }}</p><Sparkline :values="overview.daily_cost.map(day => day.estimated_cost_usd)" /></article>
        <article class="card metric-card recorder-card" :class="{ 'first-card': firstContent }"><div class="metric-head"><span class="metric-icon"><ShieldCheck :size="17" /></span><span>{{ t.dashboard.recorder }}</span><small>{{ t.dashboard.recorderParts }}</small></div><div class="metric-value"><AnimatedNumber :initial-animation="reveal" :value="recorderCount" /><span>/ 2</span></div><p :class="{ 'warning-text': overview.recorder.statusline_modified || overview.recorder.hooks_modified }">{{ recorderInfo }}</p></article>
      </section>
      <section class="middle-grid"><article class="card trend-card"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><TrendingUp :size="17" /></span><strong>{{ t.dashboard.trend(selectedTool) }}</strong><div v-if="trendTools.length > 1" v-selection-slide class="tool-switch"><button v-for="tool in trendTools" :key="tool" :class="{ active: selectedTool === tool }" @click="selectTrendTool(tool)">{{ tool }}</button></div></div><div v-selection-slide class="segmented"><button v-for="item in ([['five_hour', t.dashboard.rangeFiveHour], ['today', t.dashboard.today], ['week', t.dashboard.rangeWeek]] as const)" :key="item[0]" :class="{ active: selectedRange === item[0] }" @click="selectedRange = item[0]">{{ item[1] }}</button></div></div><QuotaTrend :overview="overview" :tool="selectedTool" :range="selectedRange" /><div class="chart-legend"><span class="legend-dot" />{{ t.dashboard.legend }}</div></article>
        <article class="card advice-card"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><Lightbulb :size="17" /></span><strong>{{ t.dashboard.adviceTitle }}</strong></div><small v-if="adviceCount" class="advice-card-count">{{ t.dashboard.adviceCount(adviceCount) }}</small></div>
          <template v-if="topAdvice.length">
            <p v-if="adviceHeadline" class="advice-card-summary"><span>{{ t.dashboard.saving }}</span><strong><AnimatedNumber :initial-animation="reveal" :value="adviceHeadline.value" /></strong><span>{{ adviceHeadline.unit }}</span></p>
            <div class="advice-card-rows">
              <button v-for="item in topAdvice" :key="item.key" class="advice-card-row" @click="router.push('/advice')">
                <span>{{ item.title }}</span>
                <span class="advice-tag" :class="item.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ adviceImpact(item) }}</span>
                <ChevronRight :size="18" />
              </button>
            </div>
          </template>
          <div v-else class="advice-empty"><Lightbulb :size="27" /><p>{{ advice ? t.dashboard.adviceNone : t.dashboard.adviceAnalyzing }}</p></div>
          <button class="advice-button" @click="router.push('/advice')">{{ t.dashboard.allAdvice }}</button></article></section>
      <section class="card sessions-card"><div class="sessions-top"><div v-selection-slide class="summary-pills"><button :class="{ active: selectedList === 'sessions' }" @click="selectedList = 'sessions'">{{ t.dashboard.sessionsCount(overview.summary.sessions) }}</button><button>{{ t.dashboard.requestsCount(formatInteger(overview.summary.requests)) }}</button><button :class="{ active: selectedList === 'prompts' }" @click="selectedList = 'prompts'">{{ t.dashboard.highPrompts(overview.summary.high_prompts) }}</button></div><button class="all-link" @click="router.push('/sessions')">{{ t.dashboard.viewAll }}</button></div><div class="table-scroll"><div class="table-head"><span>{{ t.common.source }}</span><span>{{ t.common.session }}</span><span>{{ t.common.model }}</span><span>{{ t.common.token }}</span><span>{{ t.common.cacheHit }}</span><span>{{ t.common.quota }}</span></div><button v-for="row in tableRows" :key="`${row.tool}-${row.session_id}-${'at' in row ? row.at : ''}`" class="session-row" @click="openSession(row)"><span class="row-source"><span class="row-icon"><img v-if="row.tool === 'Claude'" :src="ClaudeLogo" alt=""/><CodexLogo v-else /></span>{{ sourceLabel(row.source) }}</span><span class="truncate">{{ rowTitle(row) }}</span><span class="truncate">{{ modelLabel(row.model) }}</span><span class="numeric">{{ compactText(row.total_tokens, 1_000_000) }}</span><span class="numeric" :class="{ 'error-text': row.cache_hit_rate !== null && row.cache_hit_rate < .5 }">{{ percent(row.cache_hit_rate) }}</span><span class="numeric">{{ quotaDelta(row.weekly_delta) }}</span></button><p v-if="!tableRows.length" class="table-empty">{{ t.common.noRecords }}</p></div></section>
    </div>
    <Transition name="dialog"><ShareDialog v-if="shareOpen" @close="shareOpen = false" /></Transition>
  </div>
</template>
