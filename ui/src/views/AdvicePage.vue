<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { CircleAlert, Clock, Coins, Cpu, ExternalLink, GitFork, Layers, Lightbulb, List, Zap } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import AnimatedNumber from '../components/AnimatedNumber.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import { dismissAdvice, getAdvice, type AdviceItem, type AdviceReport, type Tool } from '../api'
import PageSkeleton from '../components/PageSkeleton.vue'
import { getPageCache, invalidatePageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usePageRequest } from '../usePageRequest'

const router = useRouter()
const period = ref<7 | 30>(pageSelection.advice.period)
const cachedReport = getPageCache<AdviceReport>(pageCacheKey('advice', period.value))
const cachedTool = cachedReport?.tools.find(item => item.items.length)?.tool ?? cachedReport?.tools[0]?.tool
const tool = ref<Tool>(pageSelection.advice.tool ?? cachedTool ?? 'Claude')
if (!pageSelection.advice.tool && cachedTool) pageSelection.advice.tool = cachedTool
const request = usePageRequest<AdviceReport>(pageCacheKey('advice', period.value))
const report = request.data
const { pending, loading, error, reveal } = request
watch([period, tool], () => {
  pageSelection.advice.period = period.value
  pageSelection.advice.tool = tool.value
})
const icons: Record<string, unknown> = {
  cache_rebuild: Clock,
  session_startup: Zap,
  long_context: Layers,
  expensive_light_task: Cpu,
  many_subagents: GitFork,
}

async function load() {
  const selectedPeriod = period.value
  const result = await request.load(pageCacheKey('advice', selectedPeriod), () => getAdvice(selectedPeriod))
  if (result && !pageSelection.advice.tool) {
    const withItems = result.tools.find(item => item.items.length)
    tool.value = withItems?.tool ?? result.tools[0]?.tool ?? 'Claude'
    pageSelection.advice.tool = tool.value
  }
}

function selectPeriod(next: 7 | 30) {
  if (next === period.value) return
  period.value = next
  void load()
}

async function dismiss(item: AdviceItem) {
  const dismissedPeriod = period.value
  const dismissedTool = tool.value
  const key = pageCacheKey('advice', dismissedPeriod)
  try {
    await dismissAdvice(item.key)
    invalidatePageCache('advice', 'sessionDetail')
    const current = report.value
    if (current && period.value === dismissedPeriod) {
      request.replace(key, {
        ...current,
        tools: current.tools.map(entry => entry.tool === dismissedTool
          ? { ...entry, hidden: entry.hidden + 1, items: entry.items.filter(row => row.key !== item.key) }
          : entry),
      })
    }
    const fresh = await getAdvice(dismissedPeriod)
    if (period.value === dismissedPeriod) request.replace(key, fresh)
    else setPageCache(key, fresh)
  } catch (cause) {
    error.value = String(cause)
  }
}

onMounted(() => { void load() })
const current = computed(() => report.value?.tools.find(item => item.tool === tool.value) ?? null)
const available = (item: Tool) => Boolean(report.value?.tools.some(entry => entry.tool === item))
const pct = (value: number) => `${value.toFixed(1)}%`
const usd = (value: number) => `$${value.toFixed(2)}`

/** "额外消耗 4.1% 周额度" for avoidable waste, "消耗 …" for necessary overhead. */
function impact(item: AdviceItem) {
  const verb = item.avoidable ? '额外消耗' : '消耗'
  return item.consumed_weekly_pct !== null
    ? `${verb} ${pct(item.consumed_weekly_pct)} 周额度`
    : `${verb}约 ${usd(item.overhead_cost_usd)}`
}
</script>

<template>
  <div class="dashboard-page advice-page" :class="{ 'no-entry-reveal': !reveal }" :style="{ '--usage-tool': tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
    <header class="dashboard-header">
      <div><h1>省额度建议</h1><p>基于近 {{ period }} 天日志识别的额度浪费，附影响程度与改进方法</p></div>
      <div class="advice-head-controls">
        <div v-selection-slide class="usage-tool-switch" aria-label="选择工具">
          <button v-for="item in (['Claude', 'Codex'] as const)" :key="item" :class="{ active: tool === item }" :disabled="!available(item)" :aria-pressed="tool === item" @click="tool = item">
            <img v-if="item === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else />{{ item }}
          </button>
        </div>
        <div v-selection-slide class="segmented advice-period" aria-label="统计范围">
          <button :class="{ active: period === 7 }" :aria-pressed="period === 7" @click="selectPeriod(7)">最近 7 天</button>
          <button :class="{ active: period === 30 }" :aria-pressed="period === 30" @click="selectPeriod(30)">最近 30 天</button>
        </div>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="advice" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !report" class="center-state"><CircleAlert :size="28" /><strong>生成建议失败</strong><p>{{ error }}</p><button @click="load()">重试</button></div>
    <div v-else-if="!current" class="center-state"><Lightbulb :size="28" /><strong>近 {{ period }} 天没有 {{ tool }} 的本地用量</strong></div>
    <template v-else>
      <p v-if="error" class="advice-error-note" role="alert">{{ error }}</p>
      <section class="advice-summary content-reveal" aria-label="建议汇总">
        <article class="card advice-stat">
          <span class="advice-stat-icon"><Coins :size="20" /></span>
          <div>
            <small>预计节省</small>
            <p v-if="current.summary.avoidable_weekly_pct !== null"><strong class="advice-saving"><AnimatedNumber :initial-animation="reveal" :value="current.summary.avoidable_weekly_pct.toFixed(1)" /></strong><span>% 周额度</span></p>
            <p v-else><strong class="advice-saving"><AnimatedNumber :initial-animation="reveal" :value="usd(current.summary.avoidable_cost_usd)" /></strong><span>API 等价</span></p>
          </div>
        </article>
        <article class="card advice-stat">
          <span class="advice-stat-icon"><Lightbulb :size="20" /></span>
          <div><small>识别出</small><p><strong>{{ current.summary.count }}</strong><span>项可优化</span></p></div>
        </article>
        <article class="card advice-stat">
          <span class="advice-stat-icon"><List :size="20" /></span>
          <div><small>涉及</small><p><strong>{{ current.summary.sessions }}</strong><span>个会话</span></p></div>
        </article>
      </section>

      <div v-if="!current.items.length" class="center-state advice-none">
        <Lightbulb :size="28" />
        <strong>近 {{ period }} 天没有发现明显的额度浪费</strong>
        <p v-if="current.hidden">已隐藏 {{ current.hidden }} 条，可在设置中恢复</p>
      </div>
      <TransitionGroup name="advice-list" tag="div" class="advice-items content-reveal">
      <article v-for="item in current.items" :key="item.key" class="card advice-item">
        <span class="advice-item-icon" :class="item.severity === 'Error' ? 'advice-error' : 'advice-warning'">
          <component :is="icons[item.id] ?? Lightbulb" :size="24" />
        </span>
        <div class="advice-item-body">
          <div class="advice-item-title">
            <h2>{{ item.title }}</h2>
            <span class="origin-pill advice-impact" :class="item.severity === 'Error' ? 'pill-error' : 'pill-warning'"><i />{{ impact(item) }}</span>
          </div>
          <div class="advice-facts"><span v-for="fact in item.facts" :key="fact">{{ fact }}</span></div>
          <p class="advice-description">{{ item.description }}</p>
          <div class="advice-actions">
            <a v-if="item.link" class="advice-link" :href="item.link.url" target="_blank" rel="noopener noreferrer"><ExternalLink :size="18" />{{ item.link.label }}</a>
            <button class="advice-dismiss" @click="dismiss(item)">不再提示</button>
            <small v-if="item.link">外部链接 · 可在配置中修改</small>
          </div>
        </div>
      </article>
      </TransitionGroup>
      <p v-if="current.items.length && current.hidden" class="advice-hidden-note">已隐藏 {{ current.hidden }} 条建议，可在<button @click="router.push('/settings')">设置</button>中恢复</p>
      <p v-if="!current.has_quota_data && current.items.length" class="advice-hidden-note">暂无 {{ tool }} 额度数据，影响程度按 API 等价费用估算。开启官方额度查询或状态栏记录器后可换算为周额度百分比。</p>
    </template>
  </div>
</template>
