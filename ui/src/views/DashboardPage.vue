<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Share, RefreshCw, CircleCheck, CircleAlert, Coins, Database, TrendingUp, ShieldCheck, Lightbulb, ChevronRight } from 'lucide-vue-next'
import { getAdvice, getOverview, refreshQuotas, rescan, type AdviceItem, type AdviceReport, type Overview, type Origin, type QuotaDisplay, type Tool, type Source } from '../api'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import Sparkline from '../components/Sparkline.vue'
import QuotaTrend from '../components/QuotaTrend.vue'
import ShareDialog from '../components/ShareDialog.vue'
import { dashboardRefreshToken, pendingDashboardNotice } from '../onboardingState'

const router = useRouter()
const overview = ref<Overview | null>(null)
const loading = ref(true)
const refreshing = ref(false)
const error = ref('')
const refreshError = ref('')
const notice = ref('')
const noticeTone = ref<'warning' | 'success'>('warning')
const selectedTool = ref<Tool>('Claude')
const selectedRange = ref<'five_hour' | 'today' | 'week'>('five_hour')
const selectedList = ref<'sessions' | 'prompts'>('sessions')
const advice = ref<AdviceReport | null>(null)
const shareOpen = ref(false)

// Advice is secondary: a failure here must not block the dashboard.
async function loadAdvice() {
  try { advice.value = await getAdvice(7) } catch { advice.value = null }
}

async function load(quiet = false) {
  if (!quiet || !overview.value) loading.value = true
  error.value = ''
  try {
    const data = await getOverview()
    overview.value = data
    void loadAdvice()
    const available = data.tools.filter(item => item.five_hour.used_percent !== null)
    selectedTool.value = available.sort((a, b) => (b.five_hour.used_percent ?? 0) - (a.five_hour.used_percent ?? 0))[0]?.tool ?? 'Claude'
  } catch (cause) {
    error.value = String(cause)
  } finally {
    loading.value = false
  }
}
async function refresh() {
  if (refreshing.value) return
  refreshing.value = true
  error.value = ''
  refreshError.value = ''
  try {
    await rescan()
    const result = await refreshQuotas()
    refreshError.value = [result.claude, result.codex].filter(message => !['已更新', '未开启', '60 秒内已查询', '开发预览'].includes(message)).join('；')
    notice.value = refreshError.value
    noticeTone.value = 'warning'
    overview.value = await getOverview()
    void loadAdvice()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    refreshing.value = false
  }
}
onMounted(() => { void load() })
watch(dashboardRefreshToken, () => {
  const pending = pendingDashboardNotice.value
  if (pending) {
    notice.value = pending.text
    noticeTone.value = pending.tone
    pendingDashboardNotice.value = null
  }
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
  if (withPct) return { value: `${(withPct.summary.avoidable_weekly_pct ?? 0).toFixed(1)}%`, unit: `${withPct.tool} 周额度` }
  const byCost = tools.sort((a, b) => b.summary.avoidable_cost_usd - a.summary.avoidable_cost_usd)[0]
  return byCost ? { value: `$${byCost.summary.avoidable_cost_usd.toFixed(2)}`, unit: 'API 等价' } : null
})
const adviceImpact = (item: AdviceItem) => item.consumed_weekly_pct !== null
  ? `${item.consumed_weekly_pct.toFixed(1)}%` : `$${item.overhead_cost_usd.toFixed(2)}`
const empty = computed(() => overview.value?.summary.sessions === 0 && overview.value?.summary.requests === 0)
const toolsWithData = computed(() => overview.value?.tools.filter(item => item.five_hour.used_percent !== null) ?? [])
const trendTools = computed(() => toolsWithData.value.map(item => item.tool))
const tableRows = computed(() => selectedList.value === 'sessions'
  ? overview.value?.recent_sessions.slice(0, 5) ?? []
  : overview.value?.recent_high_prompts.slice(0, 5) ?? [])
const time = (value: string | null, options?: Intl.DateTimeFormatOptions) => value ? new Date(value).toLocaleString('zh-CN', options ?? { hour: '2-digit', minute: '2-digit', hour12: false }) : '—'
const updatedAt = (value: string) => {
  const date = new Date(value)
  return `${date.getMonth() + 1} 月 ${date.getDate()} 日 ${time(value)}`
}
const dateSlash = (value: string) => {
  const date = new Date(value)
  return `${date.getMonth() + 1}/${date.getDate()}`
}
const percent = (value: number | null, digits = 0) => value === null ? '—' : `${(value * 100).toFixed(digits)}%`
const quotaPercent = (value: number | null) => value === null ? '—' : Number(value.toFixed(1)).toString()
const resetRemaining = (value: string | null) => {
  if (!value) return '—'
  const minutes = Math.max(0, Math.ceil((Date.parse(value) - Date.now()) / 60_000))
  return minutes >= 60 ? `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分后` : `${minutes} 分钟后`
}
const hitTime = (value: string) => {
  const minutes = Math.max(0, Math.ceil((Date.parse(value) - Date.now()) / 60_000))
  return `约 ${minutes >= 60 ? `${Math.floor(minutes / 60)} 小时 ` : ''}${minutes % 60} 分`
}
const forecast = (quota: QuotaDisplay) => {
  if (quota.window_passed) return { text: '等待新数据', sub: '等待新额度记录', tone: '' }
  if (quota.used_percent === null) return { text: '—', sub: '—', tone: '' }
  if (quota.forecast === 'Ample') return { text: '本窗口内充裕', sub: '按近 30 分钟速度', tone: 'success-text' }
  if (typeof quota.forecast === 'object') return { text: hitTime(quota.forecast.WillHit.at), sub: '后达到上限', tone: 'warning-text' }
  return { text: '数据不足', sub: '暂无预测', tone: '' }
}
const sourceText = (tool: Tool, origin: Origin | null) => {
  if (origin === 'Official') return tool === 'Claude' ? '官方数据 · 官方查询' : '官方数据 · 本地接口'
  if (origin === 'Statusline') return '官方数据 · 状态栏'
  if (origin === 'Log') return '官方数据 · 日志'
  if (origin === 'Manual') return '手动校准'
  return '暂无额度数据'
}
const planName = (tool: Tool, plan: string | null) => {
  if (!plan) return tool
  const known: Record<string, string> = tool === 'Claude'
    ? { pro: 'Claude Pro', max: 'Claude Max 5x', max_5x: 'Claude Max 5x', max_20x: 'Claude Max 20x' }
    : { plus: 'Codex · ChatGPT Plus', pro: 'Codex · ChatGPT Pro' }
  return known[plan.toLowerCase()] ?? tool
}
const compact = (value: number) => value >= 100_000_000
  ? { value: Number((value / 100_000_000).toFixed(1)).toString(), unit: '亿' }
  : value >= 10_000 ? { value: Number((value / 10_000).toFixed(value >= 1_000_000 ? 0 : 1)).toString(), unit: '万' }
  : { value: value.toLocaleString('zh-CN'), unit: '' }
const todayNumber = computed(() => compact(overview.value?.today_token_total ?? 0))
const costParts = computed(() => (overview.value?.month_estimated_cost_usd ?? 0).toFixed(2).split('.'))
const valueText = computed(() => {
  const data = overview.value
  if (!data || data.subscription_total_usd == null) return '未识别订阅套餐'
  if (!data.month_cost_by_tool) return '请重新生成本地概览数据'
  const unknown = data.tools.filter(item => item.subscription_usd == null && Object.hasOwn(data.month_cost_by_tool, item.tool))
  const note = unknown.length ? `（${unknown.map(item => item.tool).join('、')} 套餐未识别）` : ''
  return `订阅 $${data.subscription_total_usd} · 价值约 ${data.subscription_value_multiple?.toFixed(1) ?? '—'} 倍${note}`
})
const cacheHitChangeText = computed(() => {
  const change = overview.value?.cache_hit_change_from_yesterday
  if (change == null) return '暂无对比'
  if (Math.abs(change) < 0.005) return '与昨日持平'
  return `较昨日${change > 0 ? '提升' : '下降'} ${Math.abs(change * 100).toFixed(0)}%`
})
const recorderCount = computed(() => Number(overview.value?.recorder.statusline_installed) + Number(overview.value?.recorder.hooks_installed))
const recorderInfo = computed(() => {
  const recorder = overview.value?.recorder
  if (!recorder) return ''
  if (recorder.statusline_modified || recorder.hooks_modified) return '配置已被修改'
  if (!recorder.statusline_installed && !recorder.hooks_installed) return '未安装 · 可在设置中开启'
  if (!recorder.statusline_installed) return '状态栏尚未安装'
  if (!recorder.hooks_installed) return 'hooks 尚未安装'
  return '状态栏与 hooks 已安装'
})
const sourceLabel = (source: Source) => ({ ClaudeCli: 'Claude Code · 命令行', ClaudeDesktop: 'Claude · 桌面版', CodexCli: 'Codex · 命令行', CodexDesktop: 'Codex · 桌面版', Unknown: '未知来源' })[source]
const rowTitle = (row: { title: string | null; project: string | null; session_id: string }) => row.title || row.project?.split(/[\\/]/).filter(Boolean).at(-1) || row.session_id.slice(0, 8)
const quotaDelta = (value: number) => value > 0 && value < .1 ? '<0.1%' : `${value.toFixed(1)}%`
const modelLabel = (model: string) => model || '—'
const openSession = (row: { tool: Tool; session_id: string }) => {
  router.push({ path: '/sessions', query: { tool: row.tool, id: row.session_id } })
}
</script>

<template>
  <div class="dashboard-page">
    <header class="dashboard-header">
      <div><h1>仪表盘</h1><p v-if="overview">{{ updatedAt(overview.generated_at) }} 更新 · 共 {{ overview.summary.sessions }} 个会话</p><p v-else>本地用量概况</p></div>
      <div class="header-actions">
        <button class="round-action" title="分享" aria-label="分享" @click="shareOpen = true"><Share :size="21" /></button>
        <button class="round-action" title="刷新" aria-label="刷新" :disabled="refreshing || loading" @click="refresh"><RefreshCw :size="21" :class="{ spinning: refreshing }" /></button>
        <button class="round-action state-action" :class="error || refreshError ? 'state-warning' : 'state-success'" :title="error || refreshError || '数据已就绪'" aria-label="数据状态" @click="notice = error || refreshError || '数据已就绪'">
          <CircleAlert v-if="error || refreshError" :size="22" /><CircleCheck v-else :size="22" />
        </button>
      </div>
    </header>
    <div v-if="notice" class="notice" :class="{ 'notice-success': noticeTone === 'success' }" role="status">{{ notice }} <button @click="notice = ''" aria-label="关闭提示">×</button></div>
    <div v-if="loading" class="skeleton-layout" aria-label="正在加载数据"><div class="skeleton" v-for="i in 9" :key="i" /></div>
    <div v-else-if="error && !overview" class="center-state"><CircleAlert :size="30" /><strong>加载概览失败</strong><p>{{ error }}</p><button @click="load()">重试</button></div>
    <div v-else-if="empty" class="center-state"><Database :size="32" /><strong>未找到 Claude Code 或 Codex 的本地日志</strong><p>开始使用后可在此查看额度和用量。</p><button @click="refresh">重新扫描</button></div>
    <template v-else-if="overview">
      <section class="quota-grid" aria-label="订阅额度">
        <article v-for="item in overview.tools" :key="item.tool" class="card quota-card">
          <div class="quota-body">
            <div class="quota-top"><span class="tool-icon"><img v-if="item.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span><span class="plan-label">{{ planName(item.tool, item.plan) }}</span><span class="origin-pill" :class="item.five_hour.origin === 'Manual' ? 'pill-warning' : item.five_hour.used_percent === null || item.five_hour.origin === 'Estimated' ? 'pill-neutral' : 'pill-success'">{{ sourceText(item.tool, item.five_hour.origin) }}</span></div>
            <div class="quota-main"><div><div class="eyebrow">5 小时额度已用</div><div class="quota-number" :class="{ 'quota-passed': item.five_hour.window_passed }">{{ item.five_hour.window_passed ? '已重置' : quotaPercent(item.five_hour.used_percent) }}<span v-if="!item.five_hour.window_passed && item.five_hour.used_percent !== null">%</span></div></div>
              <div class="quota-ring" :class="item.tool === 'Claude' ? 'ring-claude' : 'ring-codex'"><svg viewBox="0 0 96 96" aria-hidden="true"><circle cx="48" cy="48" r="42" fill="none" stroke="var(--surface-container-high)" stroke-width="10"/><circle v-if="item.five_hour.used_percent !== null && !item.five_hour.window_passed" cx="48" cy="48" r="42" fill="none" :stroke="item.tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)'" stroke-width="10" :stroke-dasharray="`${Math.max(0, 100 - item.five_hour.used_percent) * 2.639} 264`" transform="rotate(-90 48 48)" /></svg><div class="ring-label"><strong>{{ item.five_hour.used_percent === null ? '—' : `${Math.max(0, 100 - item.five_hour.used_percent).toFixed(0)}%` }}</strong><span>剩余</span></div></div>
            </div>
            <div class="quota-facts"><div><span>下次重置</span><strong>{{ time(item.five_hour.resets_at) }}</strong><small>{{ resetRemaining(item.five_hour.resets_at) }}</small></div><div><span>本周额度</span><strong>{{ item.weekly.used_percent === null ? '—' : `${quotaPercent(item.weekly.used_percent)}%` }}</strong><small>{{ item.weekly.resets_at ? `${dateSlash(item.weekly.resets_at)} 重置` : '—' }}</small></div><div><span>用量预测</span><strong :class="forecast(item.five_hour).tone" class="forecast-text">{{ forecast(item.five_hour).text }}</strong><small>{{ forecast(item.five_hour).sub }}</small></div></div>
          </div>
          <div class="quota-footer"><button v-if="item.five_hour.used_percent === null && !item.five_hour.window_passed" class="settings-link" @click="router.push('/settings')">在设置中开启额度来源 <ChevronRight :size="14" /></button></div>
        </article>
      </section>
      <section class="metric-grid" aria-label="今日指标">
        <article class="card metric-card"><div class="metric-head"><span class="metric-icon"><Coins :size="17" /></span><span>今日用量</span><small>峰值 {{ overview.peak_hour === null ? '—' : `${String(overview.peak_hour).padStart(2, '0')}:00` }}</small></div><div class="metric-value">{{ todayNumber.value }}<span>{{ todayNumber.unit }}</span></div><p>{{ overview.today_cache_read_share === null ? '暂无缓存读取数据' : `${percent(overview.today_cache_read_share)} 来自缓存读取` }}</p><Sparkline :values="overview.hourly_tokens" /></article>
        <article class="card metric-card"><div class="metric-head"><span class="metric-icon"><Database :size="17" /></span><span>缓存命中率</span><small>今天</small></div><div class="metric-value">{{ overview.today_cache_hit_rate === null ? '—' : (overview.today_cache_hit_rate * 100).toFixed(0) }}<span v-if="overview.today_cache_hit_rate !== null">%</span></div><p>{{ cacheHitChangeText }}</p><Sparkline :values="overview.daily_cache_hit_rate.map(day => day.rate)" /></article>
        <article class="card metric-card"><div class="metric-head"><span class="metric-icon"><TrendingUp :size="17" /></span><span>本月折合 API 费用</span><small>估算</small></div><div class="metric-value">${{ costParts[0] }}<span>.{{ costParts[1] }}</span></div><p>{{ valueText }}</p><Sparkline :values="overview.daily_cost.map(day => day.estimated_cost_usd)" /></article>
        <article class="card metric-card recorder-card"><div class="metric-head"><span class="metric-icon"><ShieldCheck :size="17" /></span><span>记录器</span><small>状态栏 + hooks</small></div><div class="metric-value">{{ recorderCount }}<span>/ 2</span></div><p :class="{ 'warning-text': overview.recorder.statusline_modified || overview.recorder.hooks_modified }">{{ recorderInfo }}</p></article>
      </section>
      <section class="middle-grid"><article class="card trend-card"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><TrendingUp :size="17" /></span><strong>额度走势</strong><div class="tool-switch" v-if="trendTools.length > 1"><button v-for="tool in trendTools" :key="tool" :class="{ active: selectedTool === tool }" @click="selectedTool = tool">{{ tool }}</button></div><span v-else-if="trendTools.length">{{ trendTools[0] }}</span></div><div class="segmented"><button v-for="item in ([['five_hour', '5 小时'], ['today', '今天'], ['week', '本周']] as const)" :key="item[0]" :class="{ active: selectedRange === item[0] }" @click="selectedRange = item[0]">{{ item[1] }}</button></div></div><QuotaTrend :overview="overview" :tool="selectedTool" :range="selectedRange" /><div class="chart-legend"><span class="legend-dot" />圆点代表单次提示及其额度增量</div></article>
        <article class="card advice-card"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><Lightbulb :size="17" /></span><strong>省额度建议</strong></div><small v-if="adviceCount" class="advice-card-count">{{ adviceCount }} 条</small></div>
          <template v-if="topAdvice.length">
            <p v-if="adviceHeadline" class="advice-card-summary"><span>预计可节省</span><strong>{{ adviceHeadline.value }}</strong><span>{{ adviceHeadline.unit }}</span></p>
            <div class="advice-card-rows">
              <button v-for="item in topAdvice" :key="item.key" class="advice-card-row" @click="router.push('/advice')">
                <img v-if="item.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else class="tool-mark" />
                <span>{{ item.title }}</span>
                <span class="advice-tag" :class="item.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ adviceImpact(item) }}</span>
                <ChevronRight :size="18" />
              </button>
            </div>
          </template>
          <div v-else class="advice-empty"><Lightbulb :size="27" /><p>{{ advice ? '近 7 天没有发现明显的额度浪费' : '正在分析近 7 天的用量' }}</p></div>
          <button class="advice-button" @click="router.push('/advice')">查看全部建议</button></article></section>
      <section class="card sessions-card"><div class="sessions-top"><div class="summary-pills"><button :class="{ active: selectedList === 'sessions' }" @click="selectedList = 'sessions'">{{ overview.summary.sessions }} 个会话</button><button>{{ overview.summary.requests.toLocaleString('zh-CN') }} 次请求</button><button :class="{ active: selectedList === 'prompts' }" @click="selectedList = 'prompts'">{{ overview.summary.high_prompts }} 条高耗提示</button></div><button class="all-link" @click="router.push('/sessions')">查看全部</button></div><div class="table-scroll"><div class="table-head"><span>来源</span><span>会话</span><span>模型</span><span>token</span><span>缓存命中</span><span>额度</span></div><button v-for="row in tableRows" :key="`${row.tool}-${row.session_id}-${'at' in row ? row.at : ''}`" class="session-row" @click="openSession(row)"><span class="row-source"><span class="row-icon"><img v-if="row.tool === 'Claude'" :src="ClaudeLogo" alt=""/><CodexLogo v-else /></span>{{ sourceLabel(row.source) }}</span><span class="truncate">{{ rowTitle(row) }}</span><span class="truncate">{{ modelLabel(row.model) }}</span><span class="numeric">{{ compact(row.total_tokens).value }} {{ compact(row.total_tokens).unit }}</span><span class="numeric" :class="{ 'error-text': row.cache_hit_rate !== null && row.cache_hit_rate < .5 }">{{ percent(row.cache_hit_rate) }}</span><span class="numeric">{{ quotaDelta(row.weekly_delta) }}</span></button><p v-if="!tableRows.length" class="table-empty">暂无记录</p></div></section>
    </template>
    <ShareDialog v-if="shareOpen" @close="shareOpen = false" />
  </div>
</template>
