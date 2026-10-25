<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Overview, Tool } from '../api'
import { placeMarkerLabels, quotaTicks } from './quotaChart'
const props = defineProps<{ overview: Overview; tool: Tool; range: 'five_hour' | 'today' | 'week' }>()
const hovered = ref<string | null>(null)
const trend = computed(() => props.overview.trends.find(item => item.tool === props.tool))
const values = computed(() => trend.value?.[props.range] ?? [])
const markers = computed(() => {
  const key = props.range === 'five_hour' ? 'high_prompts' : props.range === 'today' ? 'today_high_prompts' : 'week_high_prompts'
  return [...(trend.value?.[key] ?? [])].sort((a, b) => b.delta - a.delta).slice(0, 6).sort((a, b) => Date.parse(a.at) - Date.parse(b.at))
})
const start = computed(() => {
  if (props.range === 'five_hour') return Date.parse(trend.value?.five_hour_started_at ?? values.value[0]?.at ?? props.overview.generated_at)
  if (props.range === 'week') return Date.parse(trend.value?.week_started_at ?? values.value[0]?.at ?? props.overview.generated_at)
  const d = new Date(props.overview.generated_at); d.setHours(0, 0, 0, 0); return d.getTime()
})
const end = computed(() => props.range === 'five_hour' ? start.value + 5 * 3600_000 :
  props.range === 'week' ? start.value + 7 * 24 * 3600_000 : start.value + 24 * 3600_000)
const yTicks = computed(() => quotaTicks(Math.max(0, ...values.value.map(point => point.percent))))
const max = computed(() => yTicks.value.at(-1) ?? 40)
const x = (at: string) => 38 + Math.max(0, Math.min(1, (Date.parse(at) - start.value) / (end.value - start.value))) * 552
const y = (percent: number) => 170 - Math.max(0, Math.min(1, percent / max.value)) * 138
const line = computed(() => values.value.map((point, index) => `${index ? 'L' : 'M'} ${x(point.at).toFixed(1)} ${y(point.percent).toFixed(1)}`).join(' '))
const area = computed(() => values.value.length ? `${line.value} L ${x(values.value.at(-1)!.at).toFixed(1)} 170 L ${x(values.value[0]!.at).toFixed(1)} 170 Z` : '')
const markerY = (at: string) => {
  const points = values.value
  if (!points.length) return 170
  const atMs = Date.parse(at)
  const next = points.findIndex(point => Date.parse(point.at) >= atMs)
  if (next <= 0) return y(points[0]!.percent)
  const before = points[next - 1]!
  const after = points[next] ?? before
  const fraction = (atMs - Date.parse(before.at)) / Math.max(1, Date.parse(after.at) - Date.parse(before.at))
  return y(before.percent + (after.percent - before.percent) * fraction)
}
const ticks = computed(() => Array.from({ length: 6 }, (_, index) => {
  if (props.range === 'five_hour') return `+${index}h`
  const time = new Date(start.value + index * (end.value - start.value) / 5)
  return props.range === 'today' ? `${String(time.getHours()).padStart(2, '0')}:00` : `${time.getMonth() + 1}/${time.getDate()}`
}))
const markerLabels = computed(() => placeMarkerLabels(markers.value.map(marker =>
  ({ x: x(marker.at), y: markerY(marker.at), delta: marker.delta }))))
const currentX = computed(() => 38 + Math.max(0, Math.min(1, (Date.parse(props.overview.generated_at) - start.value) / (end.value - start.value))) * 552)
const time = (at: string) => new Date(at).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
</script>
<template>
  <div class="trend-chart" :class="tool === 'Claude' ? 'claude' : 'codex'">
    <Transition name="content-fade"><svg :key="`${tool}-${range}`" viewBox="0 0 610 198" preserveAspectRatio="none" role="img" :aria-label="`${tool} ${range}额度走势`">
      <g v-for="tick in yTicks" :key="tick">
        <line x1="38" :y1="y(tick)" x2="590" :y2="y(tick)" stroke="var(--outline-variant)" />
        <text x="0" :y="y(tick) + 4" class="axis">{{ tick }}%</text>
      </g>
      <path v-if="values.length" :d="area" fill="var(--trend-color)" fill-opacity="0.18" />
      <path v-if="values.length" :d="line" fill="none" stroke="var(--trend-color)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" />
      <line :x1="currentX" y1="32" :x2="currentX" y2="170" stroke="var(--on-surface-muted)" stroke-width="1" opacity="0.65" />
      <g v-for="(marker, index) in markers" :key="`${marker.session_id}-${marker.at}`"
        @mouseenter="hovered = `${marker.session_id}-${marker.at}`" @mouseleave="hovered = null">
        <circle :cx="x(marker.at)" :cy="markerY(marker.at)" r="9" fill="transparent" />
        <circle :cx="x(marker.at)" :cy="markerY(marker.at)" r="4.5" fill="var(--surface-container)" stroke="var(--trend-color)" stroke-width="2" />
        <text v-if="markerLabels[index] !== null" :x="x(marker.at)" :y="markerLabels[index]!" text-anchor="middle" class="marker-label">+{{ marker.delta.toFixed(1) }}</text>
      </g>
      <text v-for="(tick, index) in ticks" :key="tick" :x="38 + index * 110.4" y="190" text-anchor="middle" class="axis">{{ tick }}</text>
    </svg></Transition>
    <div v-for="marker in markers" v-show="hovered === `${marker.session_id}-${marker.at}`" :key="`tip-${marker.session_id}-${marker.at}`"
      class="trend-tip" :style="{ left: `${x(marker.at) / 610 * 100}%`, top: `${Math.max(4, markerY(marker.at) - 75) / 198 * 100}%` }">
      <strong>{{ time(marker.at) }}</strong><span>{{ marker.title || marker.session_id.slice(0, 8) }}</span><b>+{{ marker.delta.toFixed(1) }}%</b>
    </div>
    <p v-if="!values.length" class="no-trend">暂无额度走势</p>
  </div>
</template>
<style scoped>
.trend-chart { position: relative; height: 198px; --trend-color: var(--tool-claude); }
.trend-chart.codex { --trend-color: var(--tool-codex); }
svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
.axis { fill: var(--on-surface-muted); font-size: 10px; }
.marker-label { fill: var(--on-surface); font-size: 10px; font-weight: 600; }
.trend-tip { position: absolute; z-index: 2; display: grid; gap: 3px; min-width: 120px; padding: 8px 10px; border: 1px solid var(--outline); border-radius: 9px; background: var(--surface-container-highest); box-shadow: 0 8px 20px var(--surface-bg); color: var(--on-surface); font-size: 11px; pointer-events: none; transform: translateX(-50%); }
.trend-tip b { color: var(--trend-color); }
.no-trend { position: absolute; inset: 60px 0 auto; text-align: center; color: var(--on-surface-muted); font-size: 12px; }
</style>
