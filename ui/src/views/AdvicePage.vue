<script setup lang="ts">
import { formatInteger, messages, m } from '../i18n/index.ts'
const t = messages
import { showToast } from '../toast'
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowLeftRight, Brain, CircleAlert, Clock, Coins, Copy, Cpu, ExternalLink, FileStack, Gauge, GitFork, Layers, Lightbulb, List, Network, Search, Sparkles, TrendingUp, WandSparkles, Zap } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from '../components/CodexLogo.vue'
import AnimatedNumber from '../components/AnimatedNumber.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import { dismissAdvice, getAdvice, type AdviceItem, type AdviceReport, type Tool } from '../api'
import PageSkeleton from '../components/PageSkeleton.vue'
import { getPageCache, invalidatePageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { usePageRequest } from '../usePageRequest'
import { usageRevision } from '../realtime'

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
  model_switch: ArrowLeftRight,
  context_spike: FileStack,
  runaway_prompt: TrendingUp,
  expensive_subagent: Cpu,
  heavy_reasoning: Brain,
  fast_mode: Gauge,
  high_effort_light_task: Sparkles,
  frequent_compaction: Layers,
  web_search: Search,
  quota_rebalance: Network,
  plan_fit: WandSparkles,
}

async function load(keep = false) {
  const selectedPeriod = period.value
  const result = await request.load(pageCacheKey('advice', selectedPeriod), () => getAdvice(selectedPeriod), { keep })
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
    showToast(cause instanceof Error ? cause.message : String(cause), 'error')
  }
}

/** Uses the WebView clipboard; no clipboard plugin or extra capability. */
async function copyCommand(command: string) {
  try {
    if (!navigator.clipboard) throw new Error('clipboard unavailable')
    await navigator.clipboard.writeText(command)
    showToast(m().advice.copied(command), 'success')
  } catch {
    showToast(m().advice.copyFailed, 'warning')
  }
}

onMounted(() => { void load() })
watch(usageRevision, () => { void load(true) })
const current = computed(() => report.value?.tools.find(item => item.tool === tool.value) ?? null)
const available = (item: Tool) => Boolean(report.value?.tools.some(entry => entry.tool === item))
const pct = (value: number) => `${value.toFixed(1)}%`
const usd = (value: number) => `$${value.toFixed(2)}`

/** "额外消耗 4.1% 周额度" for avoidable waste, "消耗 …" for necessary overhead. */
function impact(item: AdviceItem) {
  if (!item.avoidable && item.overhead_cost_usd === 0) {
    return item.overhead_tokens ? m().advice.impactTokens(formatInteger(item.overhead_tokens)) : item.sessions === 0 ? m().advice.impactPlan : m().advice.impactUsage
  }
  if (item.consumed_weekly_pct !== null) return item.avoidable ? m().advice.impactExtraWeekly(pct(item.consumed_weekly_pct)) : m().advice.impactWeekly(pct(item.consumed_weekly_pct))
  return item.avoidable ? m().advice.impactExtraCost(usd(item.overhead_cost_usd)) : m().advice.impactCost(usd(item.overhead_cost_usd))
}
</script>

<template>
  <div class="dashboard-page advice-page" :class="{ 'no-entry-reveal': !reveal }" :style="{ '--usage-tool': tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)' }">
    <header class="dashboard-header">
      <div><h1>{{ t.pages.advice.title }}</h1><p>{{ t.advice.subtitle(period) }}</p></div>
      <div class="advice-head-controls">
        <div v-selection-slide class="usage-tool-switch" :aria-label="t.advice.selectTool">
          <button v-for="item in (['Claude', 'Codex'] as const)" :key="item" :class="{ active: tool === item }" :disabled="!available(item)" :aria-pressed="tool === item" @click="tool = item">
            <img v-if="item === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else />{{ item }}
          </button>
        </div>
        <div v-selection-slide class="segmented advice-period" :aria-label="t.advice.period">
          <button :class="{ active: period === 7 }" :aria-pressed="period === 7" @click="selectPeriod(7)">{{ t.advice.last7 }}</button>
          <button :class="{ active: period === 30 }" :aria-pressed="period === 30" @click="selectPeriod(30)">{{ t.advice.last30 }}</button>
        </div>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="advice" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="error && !report" class="center-state"><CircleAlert :size="28" /><strong>{{ t.advice.loadFailed }}</strong><p>{{ error }}</p><button @click="load()">{{ t.common.retry }}</button></div>
    <div v-else-if="!current" class="center-state"><Lightbulb :size="28" /><strong>{{ t.advice.noUsage(period, tool) }}</strong></div>
    <template v-else>
      <section class="advice-summary content-reveal" :aria-label="t.advice.summary">
        <article class="card advice-stat">
          <span class="advice-stat-icon"><Coins :size="20" /></span>
          <div>
            <small>{{ t.advice.expectedSaving }}</small>
            <p v-if="current.summary.avoidable_cost_usd === 0 && !current.summary.avoidable_weekly_pct"><strong class="advice-saving">—</strong><span>{{ t.advice.notQuantified }}</span></p>
            <p v-else-if="current.summary.avoidable_weekly_pct !== null"><strong class="advice-saving"><AnimatedNumber :initial-animation="reveal" :value="current.summary.avoidable_weekly_pct.toFixed(1)" /></strong><span>{{ t.advice.weeklyPctUnit }}</span></p>
            <p v-else><strong class="advice-saving"><AnimatedNumber :initial-animation="reveal" :value="usd(current.summary.avoidable_cost_usd)" /></strong><span>{{ t.common.apiEquivalent }}</span></p>
          </div>
        </article>
        <article class="card advice-stat">
          <span class="advice-stat-icon"><Lightbulb :size="20" /></span>
          <div><small>{{ t.advice.identified }}</small><p><strong>{{ current.summary.count }}</strong><span>{{ t.advice.optimizable }}</span></p></div>
        </article>
        <article class="card advice-stat">
          <span class="advice-stat-icon"><List :size="20" /></span>
          <div v-if="current.summary.sessions"><small>{{ t.advice.involves }}</small><p><strong>{{ current.summary.sessions }}</strong><span>{{ t.advice.sessionsUnit(current.summary.sessions) }}</span></p></div>
          <div v-else><small>{{ t.advice.basis }}</small><p><strong>{{ t.advice.basisQuota }}</strong><span>{{ t.advice.basisStatus }}</span></p></div>
        </article>
      </section>

      <div v-if="!current.items.length" class="center-state advice-none">
        <Lightbulb :size="28" />
        <strong>{{ t.advice.noneFound(period) }}</strong>
        <p v-if="current.hidden">{{ t.advice.hiddenRestore(current.hidden) }}</p>
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
          <div v-if="item.next_step" class="advice-next-step">
            <span class="advice-next-label">{{ t.advice.nextStep }}</span>
            <span class="advice-next-text">{{ item.next_step.text }}</span>
            <template v-if="item.next_step.command">
              <code>{{ item.next_step.command }}</code>
              <button class="advice-copy" :aria-label="t.advice.copyCommand" :title="t.advice.copyCommand" @click="copyCommand(item.next_step.command)"><Copy :size="14" />{{ t.advice.copyCommand }}</button>
            </template>
          </div>
          <div class="advice-actions">
            <a v-if="item.link" class="advice-link" :href="item.link.url" target="_blank" rel="noopener noreferrer"><ExternalLink :size="18" />{{ item.link.label }}</a>
            <button class="advice-dismiss" @click="dismiss(item)">{{ t.advice.dismiss }}</button>
            <small v-if="item.link">{{ t.advice.externalLink }}</small>
          </div>
        </div>
      </article>
      </TransitionGroup>
      <p v-if="current.items.length && current.hidden" class="advice-hidden-note">{{ t.advice.hiddenNoteBefore(current.hidden) }}<button @click="router.push('/settings')">{{ t.advice.hiddenNoteLink }}</button>{{ t.advice.hiddenNoteAfter }}</p>
      <p v-if="!current.has_quota_data && current.items.some(item => item.overhead_cost_usd > 0)" class="advice-hidden-note">{{ t.advice.noQuotaNote(tool) }}</p>
    </template>
  </div>
</template>
