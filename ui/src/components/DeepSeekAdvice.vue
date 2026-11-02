<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Calculator, CircleAlert, Coins, Database, EyeOff, Lightbulb, List } from 'lucide-vue-next'
import { dismissAdvice, getDeepSeekAdvice, type DeepSeekAdvice, type DeepSeekTip } from '../api'
import AnimatedNumber from './AnimatedNumber.vue'
import PageSkeleton from './PageSkeleton.vue'
import { yuan } from '../deepseekLogic'
import { getPageCache, invalidatePageCache, pageCacheKey, setPageCache } from '../pageCache'
import { usageRevision } from '../realtime'
import { showToast } from '../toast'
import { formatInteger, m, messages } from '../i18n/index.ts'

const props = defineProps<{ period: 7 | 30; reveal: boolean }>()
const t = messages
const key = (period: number) => pageCacheKey('advice', 'deepseek', period)
const advice = ref<DeepSeekAdvice | null>(getPageCache<DeepSeekAdvice>(key(props.period)) ?? null)
const pending = ref(advice.value === null)
const error = ref('')
let request = 0

async function load(quiet = false) {
  const ticket = ++request
  const period = props.period
  const cached = getPageCache<DeepSeekAdvice>(key(period))
  if (cached) advice.value = cached
  else if (!quiet) pending.value = true
  error.value = ''
  try {
    const result = await getDeepSeekAdvice(period)
    setPageCache(key(period), result)
    if (ticket === request) advice.value = result
  } catch (cause) {
    if (ticket === request && !cached) error.value = String(cause)
  } finally {
    if (ticket === request) pending.value = false
  }
}
onMounted(() => { void load(advice.value !== null) })
watch(() => props.period, () => { void load() })
watch(usageRevision, () => { void load(true) })

async function dismiss(tip: DeepSeekTip) {
  try {
    await dismissAdvice(tip.key)
    invalidatePageCache('advice')
    if (advice.value) advice.value = { ...advice.value, tips: advice.value.tips.filter(item => item.key !== tip.key) }
    await load(true)
  } catch (cause) {
    showToast(cause instanceof Error ? cause.message : String(cause), 'error')
  }
}

const icons = { log_upload: EyeOff, title_requests: Calculator, cold_start: Database } as const
const tone = (tip: DeepSeekTip) => tip.kind === 'log_upload' ? 'info' : 'warning'
const copy = (tip: DeepSeekTip) => m().deepseek.tips[tip.kind]
const tag = (tip: DeepSeekTip) => tip.kind === 'title_requests' ? m().deepseek.tips.title_requests.tag(yuan(tip.cost_per_day)) : copy(tip).tag as string
function chips(tip: DeepSeekTip): string[] {
  if (tip.kind === 'log_upload') return m().deepseek.tips.log_upload.chips(tip.sessions)
  if (tip.kind === 'title_requests') return m().deepseek.tips.title_requests.chips(tip.requests, tip.model)
  return m().deepseek.tips.cold_start.chips(tip.sessions, formatInteger(tip.tokens))
}
const saving = computed(() => advice.value && advice.value.saving_per_day > 0 ? yuan(advice.value.saving_per_day) : null)
</script>

<template>
  <PageSkeleton v-if="pending && !advice" kind="advice" />
  <div v-else-if="error && !advice" class="center-state"><CircleAlert :size="28" /><strong>{{ t.advice.loadFailed }}</strong><p>{{ error }}</p><button @click="load()">{{ t.common.retry }}</button></div>
  <template v-else-if="advice">
    <section class="advice-summary content-reveal" :aria-label="t.advice.summary">
      <article class="card advice-stat">
        <span class="advice-stat-icon"><Coins :size="20" /></span>
        <div><small>{{ t.deepseek.saving }}</small>
          <p v-if="saving"><strong class="advice-saving"><AnimatedNumber :initial-animation="reveal" :value="saving" /></strong><span>{{ t.deepseek.perDay }}</span></p>
          <p v-else><strong class="advice-saving">—</strong><span>{{ t.advice.notQuantified }}</span></p>
        </div>
      </article>
      <article class="card advice-stat">
        <span class="advice-stat-icon"><Lightbulb :size="20" /></span>
        <div><small>{{ t.deepseek.found }}</small><p><strong>{{ advice.tips.length }}</strong><span>{{ t.deepseek.items }}</span></p></div>
      </article>
      <article class="card advice-stat">
        <span class="advice-stat-icon"><List :size="20" /></span>
        <div><small>{{ t.deepseek.involves }}</small><p><strong>{{ advice.sessions }}</strong><span>{{ t.deepseek.sessionsUnit }}</span></p></div>
      </article>
    </section>
    <div v-if="!advice.tips.length" class="center-state advice-none"><Lightbulb :size="28" /><strong>{{ t.deepseek.noTips }}</strong></div>
    <TransitionGroup name="advice-list" tag="div" class="advice-items content-reveal">
      <article v-for="tip in advice.tips" :key="tip.key" class="card advice-item" :data-advice-key="tip.key">
        <span class="advice-item-icon" :class="`advice-${tone(tip)}`"><component :is="icons[tip.kind]" :size="24" /></span>
        <div class="advice-item-body">
          <div class="advice-item-title">
            <h2>{{ copy(tip).title }}</h2>
            <span class="origin-pill advice-impact" :class="`pill-${tone(tip)}`"><i />{{ tag(tip) }}</span>
          </div>
          <div class="advice-facts"><span v-for="fact in chips(tip)" :key="fact">{{ fact }}</span></div>
          <p class="advice-description">{{ copy(tip).body }}</p>
          <div class="advice-next-step"><span class="advice-next-label">{{ t.deepseek.next }}</span><span class="advice-next-text">{{ copy(tip).next }}</span></div>
          <div class="advice-actions"><button class="advice-dismiss" @click="dismiss(tip)">{{ t.deepseek.dismiss }}</button></div>
        </div>
      </article>
    </TransitionGroup>
  </template>
</template>
