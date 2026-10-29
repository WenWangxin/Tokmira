<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ChevronRight, Copy, Lightbulb, TriangleAlert } from 'lucide-vue-next'
import { getUsageDetail, getUsageWindows, type AdviceItem, type UsageDetail, type UsageWindowRef } from '../api'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from './CodexLogo.vue'
import { copyCommand } from '../clipboard'
import { clock, messages, monthDay } from '../i18n/index.ts'
import { cacheVersion, cacheVersionMatches, getPageCache, pageCacheKey, pageSelection, setPageCache } from '../pageCache'
import { hitRows, type QuotaHit } from '../quotaHit'

const props = defineProps<{
  hit: QuotaHit
  /** Same wording as the quota card's source pill. */
  source: string
  advice: AdviceItem | null
  /** Changes on every dashboard reload, so the ranking follows the overview without its own timer. */
  refreshKey: string
}>()
const t = messages
const router = useRouter()
const usageWindow = ref<UsageWindowRef | null>(null)
const detail = ref<UsageDetail | null>(null)
const failed = ref(false)
let ticket = 0

// Only mounted while a hit exists, so the dashboard asks for usage detail only then.
async function load() {
  const request = ++ticket
  try {
    const windows = await getUsageWindows(props.hit.tool)
    const current = windows.find(item => item.kind === props.hit.kind && item.is_current)
    if (!current) throw new Error('no current window')
    const key = pageCacheKey('usageDetail', current.tool, current.kind, current.window_id)
    const cached = getPageCache<UsageDetail>(key)
    if (cached && request === ticket && !detail.value) { usageWindow.value = current; detail.value = cached }
    const version = cacheVersion(key)
    const result = await getUsageDetail(current.tool, current.kind, current.window_id)
    if (cacheVersionMatches(key, version)) setPageCache(key, result)
    if (request !== ticket) return
    usageWindow.value = current
    detail.value = result
    failed.value = false
  } catch {
    // Keep the last ranking during quiet refreshes; show the failure only when there is none.
    if (request === ticket && !detail.value) failed.value = true
  }
}
watch(() => [props.hit.tool, props.hit.kind, props.hit.resets_at], () => {
  usageWindow.value = null
  detail.value = null
  failed.value = false
})
watch(() => [props.hit.tool, props.hit.kind, props.hit.resets_at, props.refreshKey], () => { void load() }, { immediate: true })

const rows = computed(() => detail.value ? hitRows(detail.value) : [])
const percent = (value: number) => `${Number(value.toFixed(1))}%`
const readingTime = computed(() => {
  const at = new Date(props.hit.as_of)
  return at.toDateString() === new Date().toDateString() ? clock(at) : `${monthDay(at)} ${clock(at)}`
})
const title = computed(() => t.value.dashboard.hit.title(props.hit.tool,
  props.hit.kind === 'FiveHour' ? t.value.dashboard.hit.fiveHour : t.value.dashboard.hit.weekly,
  Number(props.hit.reading.toFixed(1)).toString(), props.source, readingTime.value))

function openUsage(options: { help?: boolean; dimension?: 'subagents' } = {}) {
  pageSelection.usage.tool = props.hit.tool
  if (usageWindow.value) pageSelection.usage.windowIds[props.hit.tool] = usageWindow.value.window_id
  if (options.dimension) pageSelection.usage.dimension = options.dimension
  void router.push({ path: '/usage', query: options.help ? { help: 'unattributed' } : {} })
}
function openSession(id: string) {
  void router.push({ path: '/sessions', query: { tool: props.hit.tool, id } })
}
</script>

<template>
  <article class="card quota-hit-card" role="status">
    <div class="card-heading">
      <div class="heading-title quota-hit-title"><span class="tool-icon"><img v-if="hit.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span><strong>{{ title }}</strong></div>
      <span class="origin-pill pill-warning"><TriangleAlert :size="12" />{{ t.dashboard.hit.section }}</span>
    </div>
    <p class="quota-hit-note">{{ t.dashboard.hit.readingOnly }}</p>
    <div class="quota-hit-body">
      <section class="quota-hit-section">
        <h3>{{ t.dashboard.hit.topTitle }}</h3>
        <div v-if="rows.length" class="advice-card-rows">
          <template v-for="row in rows" :key="row.kind === 'session' ? row.key : row.kind">
            <button v-if="row.kind === 'session'" class="advice-card-row" @click="openSession(row.key)"><span>{{ row.name }}</span><span class="advice-tag pill-neutral">{{ percent(row.delta) }}</span><ChevronRight :size="18" /></button>
            <button v-else-if="row.kind === 'subagent'" class="advice-card-row" @click="openUsage({ dimension: 'subagents' })"><span>{{ t.dashboard.hit.subagent }}</span><span class="advice-tag pill-neutral">{{ percent(row.delta) }}</span><ChevronRight :size="18" /></button>
            <button v-else class="advice-card-row" @click="openUsage({ help: true })"><span>{{ t.dashboard.hit.unattributed }}<small class="quota-hit-help">{{ t.dashboard.hit.whatIsUnattributed }}</small></span><span class="advice-tag pill-neutral">{{ percent(row.delta) }}</span><ChevronRight :size="18" /></button>
          </template>
        </div>
        <p v-else class="quota-hit-empty">{{ failed ? t.dashboard.hit.loadFailed : detail ? t.dashboard.hit.noRows : t.dashboard.hit.loading }}</p>
        <button class="settings-link" @click="openUsage()">{{ t.dashboard.hit.viewUsage }} <ChevronRight :size="14" /></button>
      </section>
      <section v-if="advice" class="quota-hit-section">
        <h3>{{ t.dashboard.hit.adviceTitle }}</h3>
        <button class="advice-card-row" @click="router.push({ path: '/advice' })"><Lightbulb :size="16" /><span>{{ advice.title }}</span><span class="advice-tag" :class="advice.severity === 'Error' ? 'pill-error' : 'pill-warning'">{{ percent(advice.avoidable_weekly_pct ?? 0) }}</span><ChevronRight :size="18" /></button>
        <div v-if="advice.next_step" class="advice-next-step">
          <span class="advice-next-label">{{ t.advice.nextStep }}</span>
          <span class="advice-next-text">{{ advice.next_step.text }}</span>
          <template v-if="advice.next_step.command">
            <code>{{ advice.next_step.command }}</code>
            <button class="advice-copy" :aria-label="t.advice.copyCommand" :title="t.advice.copyCommand" @click="copyCommand(advice.next_step.command)"><Copy :size="14" />{{ t.advice.copyCommand }}</button>
          </template>
        </div>
      </section>
    </div>
  </article>
</template>
