<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { ChevronRight, ExternalLink } from 'lucide-vue-next'
import { openDeepSeekTopUp, type DeepSeekOverview } from '../api'
import { showToast } from '../toast'
import ToolLogo from './ToolLogo.vue'
import AnimatedNumber from './AnimatedNumber.vue'
import { daysLeftText, daysTone, yuan } from '../deepseekLogic'
import { clock, messages } from '../i18n/index.ts'

const props = defineProps<{ overview: DeepSeekOverview; reveal: boolean; first: boolean }>()
const router = useRouter()
const t = messages

const balance = computed(() => props.overview.balance)
function topUp() {
  openDeepSeekTopUp().catch(cause => showToast(cause instanceof Error ? cause.message : String(cause), 'error'))
}
const origin = computed(() => {
  if (!balance.value) return { text: t.value.deepseek.originNone, tone: 'pill-neutral' }
  if (balance.value.origin === 'Manual') return { text: t.value.deepseek.originManual, tone: 'pill-neutral' }
  return { text: t.value.deepseek.originOfficial, tone: 'pill-success' }
})
const asOf = computed(() => {
  const value = balance.value
  if (!value) return undefined
  return value.origin === 'Manual'
    ? t.value.deepseek.manualAsOf(clock(value.as_of), yuan(value.spent_since ?? 0))
    : t.value.deepseek.asOf(clock(value.as_of))
})
const amount = computed(() => Math.max(0, balance.value?.total ?? 0).toFixed(2))
/** Topped-up and bonus shares of the bar; a manual reading has no split. */
const split = computed(() => {
  const value = balance.value
  if (!value || value.topped_up === null || value.granted === null) return null
  const sum = Math.max(0, value.topped_up) + Math.max(0, value.granted)
  return sum > 0 ? { topped: Math.max(0, value.topped_up) / sum * 100, granted: Math.max(0, value.granted) / sum * 100 } : { topped: 0, granted: 0 }
})
</script>

<template>
  <article class="card quota-card balance-card" :class="{ 'first-card': first, 'balance-low': overview.low_balance }">
    <div class="quota-top">
      <span class="tool-icon"><ToolLogo tool="DeepSeek" /></span>
      <span class="plan-label">{{ t.deepseek.cardTitle }}</span>
      <span class="origin-pill" :class="origin.tone" :title="asOf">{{ origin.text }}</span>
      <span v-if="overview.low_balance && balance" class="origin-pill pill-warning">{{ t.deepseek.lowBalance }}</span>
      <button type="button" class="balance-top-up settings-link" @click="topUp"><ExternalLink :size="15" />{{ t.deepseek.topUp }}</button>
    </div>
    <div class="balance-body">
      <div class="balance-main">
        <div class="eyebrow">{{ t.deepseek.available }}</div>
        <div v-if="balance" class="quota-number balance-number"><span class="balance-currency">¥</span><AnimatedNumber :initial-animation="reveal" :value="amount" /></div>
        <div v-else class="quota-number balance-number balance-empty">—</div>
        <template v-if="balance && split">
          <div class="balance-bar" aria-hidden="true"><span class="balance-topped" :style="{ width: `${split.topped}%` }" /><span class="balance-granted" :style="{ width: `${split.granted}%` }" /></div>
          <div class="balance-legend"><span><i class="balance-topped" />{{ t.deepseek.toppedUp(yuan(balance.topped_up)) }}</span><span><i class="balance-granted" />{{ t.deepseek.granted(yuan(balance.granted)) }}</span></div>
        </template>
        <p v-else-if="balance" class="balance-note">{{ asOf }}</p>
        <p v-if="overview.unpriced_requests" class="balance-note">{{ t.deepseek.unpriced(overview.unpriced_requests) }}</p>
      </div>
      <div class="quota-facts balance-facts">
        <div><span>{{ t.deepseek.todayCost }}</span><strong>{{ yuan(overview.today_cost) }}</strong><small>{{ t.deepseek.estimateLocal }}</small></div>
        <div><span>{{ t.deepseek.dailyAverage }}</span><strong>{{ yuan(overview.daily_average) }}</strong><small>{{ t.deepseek.byLocalLogs }}</small></div>
        <div>
          <span>{{ t.deepseek.pace }}</span>
          <strong :class="daysTone(overview)">{{ daysLeftText(overview) }}</strong>
          <small v-if="balance">{{ t.deepseek.untilEmpty }}</small>
          <button v-else class="settings-link balance-enable" @click="router.push({ path: '/settings', query: { section: 'deepseek' } })">{{ t.deepseek.enableBalance }} <ChevronRight :size="13" /></button>
        </div>
      </div>
    </div>
  </article>
</template>
