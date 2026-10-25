<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Cpu, Folder, GitFork, List, Zap, Database, CircleAlert } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import { getAdvice, getUsageDetail, getUsageWindows, promptTag, type AdviceReport, type Tool, type UsageDetail, type UsageRank, type UsageWindowRef } from '../api'
import PageSkeleton from '../components/PageSkeleton.vue'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'

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
const dimensions = [
  { key: 'sessions', name: '会话', icon: List },
  { key: 'projects', name: '项目', icon: Folder },
  { key: 'models', name: '模型', icon: Cpu },
  { key: 'subagents', name: '子代理', icon: GitFork },
] as const

async function load() {
  if (!cachedWindows) {
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
    if (!cachedWindows) error.value = String(cause)
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
const toTime = (value: string) => new Date(value).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false })
const toDay = (value: string) => { const date = new Date(value); return `${date.getMonth() + 1}/${date.getDate()}` }
function windowLabel(window: UsageWindowRef) {
  if (window.kind === 'Weekly') return '本周'
  if (window.is_current) return '当前'
  const day = new Date(window.start).toDateString()
  if (day === new Date().toDateString()) return '今天'
  const yesterday = new Date(); yesterday.setDate(yesterday.getDate() - 1)
  return day === yesterday.toDateString() ? '昨天' : toDay(window.start)
}
const windowTimes = (window: UsageWindowRef) => window.kind === 'Weekly'
  ? `${toDay(window.start)}–${toDay(window.end)}` : `${toTime(window.start)}–${toTime(window.end)}`
const percent = (value: number) => `${value.toFixed(1)}%`
const tokens = (value: number) => value >= 100_000_000 ? `${Number((value / 100_000_000).toFixed(1))} 亿`
  : value >= 10_000 ? `${Number((value / 10_000).toFixed(1))} 万` : value.toLocaleString('zh-CN')
const originText = computed(() => {
  const origin = detail.value?.origin
  if (origin === 'Official') return tool.value === 'Claude' ? '官方数据 · 官方查询' : '官方数据 · 本地接口'
  if (origin === 'Statusline') return '官方数据 · 状态栏'
  if (origin === 'Log') return '官方数据 · 日志'
  if (origin === 'Manual') return '手动校准'
  return '估算'
})
const chartSegments = computed(() => {
  if (!detail.value) return []
  const sessions = detail.value.sessions
  const parts = sessions.slice(0, 4).map((item, index) => ({ name: item.name, value: item.delta, level: index }))
  const other = sessions.slice(4).reduce((sum, item) => sum + item.delta, 0)
  if (other > 0) parts.push({ name: '其他会话', value: other, level: 4 })
  if (detail.value.unattributed_total > 0) parts.push({ name: '未归属', value: detail.value.unattributed_total, level: 5 })
  return parts
})
const chartWidth = (value: number) => `${Math.max(0, Math.min(100, value))}%`
const breakdown = computed(() => detail.value?.official_weekly_breakdown?.rows.map(row =>
  `${row.display_name || row.key} ${percent(row.percent)}`).join(' · ') ?? '')
</script>

<template>
  <div class="dashboard-page usage-page" :class="{ 'no-entry-reveal': !reveal }" :style="{ '--usage-tool': tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
    <header class="dashboard-header">
      <div><h1>用量明细</h1><p>逐一追溯额度去向，精确到会话与单条提示</p></div>
      <div v-selection-slide class="usage-tool-switch" aria-label="选择工具">
        <button v-for="item in (['Claude', 'Codex'] as const)" :key="item" :class="{ active: tool === item }" :disabled="!windows[item].length" :aria-pressed="tool === item" @click="selectTool(item)">
          <img v-if="item === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else />{{ item }}
        </button>
      </div>
    </header>
    <PageSkeleton v-if="pending && loading" kind="usage" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !selected" class="center-state"><CircleAlert :size="28" /><strong>读取用量数据失败</strong><p>{{ error }}</p><button @click="load">重试</button></div>
    <div v-else-if="!windows[tool].length" class="center-state"><Database :size="28" /><strong>暂无 {{ tool }} 额度数据</strong><p>开启官方额度查询或安装状态栏记录器后，即可按会话和提示追溯额度。</p><button @click="router.push('/settings')">前往设置</button></div>
    <template v-else>
      <nav class="usage-window-list" aria-label="额度窗口">
        <button v-for="window in windows[tool]" :key="`${window.kind}-${window.window_id}`" :class="{ active: selected?.kind === window.kind && selected?.window_id === window.window_id }" :aria-current="selected?.window_id === window.window_id ? 'true' : undefined" @click="selectWindow(window)">
          <strong>{{ windowLabel(window) }}</strong><span>{{ windowTimes(window) }}</span>
        </button>
      </nav>
      <PageSkeleton v-if="detailPending && detailLoading" kind="usageDetail" />
      <div v-else-if="detailPending" class="page-await" />
      <div v-else-if="error && !detail" class="center-state"><CircleAlert :size="28" /><strong>生成窗口明细失败</strong><p>{{ error }}</p><button @click="selectWindow(selected)">重试</button></div>
      <template v-else-if="detail">
        <section class="card usage-overview content-reveal" aria-label="窗口概览">
          <div class="usage-overview-top"><div class="usage-overview-main"><span>本窗口已用</span><strong>{{ percent(detail.used_percent) }}</strong><span class="origin-pill" :class="detail.origin === 'Manual' ? 'pill-warning' : detail.origin === 'Estimated' ? 'pill-neutral' : 'pill-success'">{{ originText }}</span></div><span class="usage-reset">{{ selected?.is_current ? `剩余 ${percent(detail.remaining_percent)}` : '已结束' }} · {{ toTime(detail.resets_at) }} 重置</span></div>
          <div class="usage-bar" role="img" :aria-label="`已用 ${percent(detail.used_percent)}，已归属 ${percent(detail.attributed_total)}，未归属 ${percent(detail.unattributed_total)}`"><span v-for="(part, index) in chartSegments" :key="`${part.name}-${index}`" :class="`usage-segment usage-level-${part.level}`" :style="{ width: barsReady ? chartWidth(part.value) : '0%' }" /><span class="usage-remainder" :style="{ width: barsReady ? chartWidth(detail.remaining_percent) : '0%' }" /></div>
          <div class="usage-legend"><span v-for="(part, index) in chartSegments" :key="`${part.name}-${index}`"><i :class="`usage-level-${part.level}`" />{{ part.name }} {{ percent(part.value) }}</span></div>
          <p v-if="breakdown" class="usage-breakdown">官方本周占比：{{ breakdown }}</p>
          <p class="usage-note">{{ breakdown ? '本机日志只覆盖 Claude Code；聊天和 Cowork 的部分以官方占比为准。' : '“未归属”指官方额度中无法与本机日志对应的部分，例如同一账号在其他设备上的使用。网页与桌面端聊天不在统计范围内。' }}</p>
        </section>
        <div class="usage-grid content-reveal">
          <section class="card usage-ranking" aria-label="用量排行"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><List :size="17" /></span><strong>用量排行</strong></div><div v-selection-slide class="segmented"><button v-for="item in dimensions" :key="item.key" :class="{ active: dimension === item.key }" :aria-pressed="dimension === item.key" @click="dimension = item.key"><component :is="item.icon" :size="16" />{{ item.name }}</button></div></div>
            <template v-if="visibleRows.length"><div v-for="row in visibleRows" :key="row.key" class="usage-rank-row"><div class="usage-rank-name"><strong :title="row.name">{{ row.name }}</strong><small>{{ row.subtitle }}</small></div><div class="usage-rank-track"><span :style="{ width: barsReady ? `${maxDelta ? Math.max(0, row.delta / maxDelta * 100) : 0}%` : '0%' }" /></div><span class="usage-rank-tokens">{{ tokens(row.tokens) }}</span><strong class="usage-rank-delta">{{ percent(row.delta) }}</strong></div><p v-if="rows.length > 8" class="usage-more">还有 {{ rows.length - 8 }} 项</p></template>
            <p v-else class="table-empty">本窗口内没有本机请求，增量全部计为未归属。</p>
          </section>
          <section class="card usage-prompts" aria-label="高耗提示"><div class="card-heading"><div class="heading-title"><span class="metric-icon"><Zap :size="17" /></span><strong>高耗提示</strong></div><small>不记录内容</small></div>
            <template v-if="topPrompts.length"><button v-for="prompt in topPrompts" :key="`${prompt.session_id}-${prompt.at}`" class="usage-prompt-row" @click="router.push({ path: '/sessions', query: { tool, id: prompt.session_id } })"><span class="usage-prompt-info"><small>{{ toTime(prompt.at) }}</small><strong :title="prompt.title">{{ prompt.title }}</strong><span v-if="tagFor(prompt.session_id, prompt.at)" class="advice-tag usage-prompt-tag" :class="tagFor(prompt.session_id, prompt.at)?.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ tagFor(prompt.session_id, prompt.at)?.tag }}</span></span><strong class="usage-prompt-delta">+{{ percent(prompt.delta) }}</strong></button></template>
            <p v-else class="table-empty">{{ detail.sessions.length ? '暂无可关联的提示记录。' : '本窗口内没有本机请求，增量全部计为未归属。' }}</p>
          </section>
        </div>
      </template>
    </template>
  </div>
</template>
