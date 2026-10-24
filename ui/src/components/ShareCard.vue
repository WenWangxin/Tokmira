<script setup lang="ts">
import { computed, ref } from 'vue'
import { brandLogoUri, claudeLogoUri, codexLogoUri } from '../shareLogos'
import {
  SHARE_FOOTER,
  costBarSegments,
  formatAmountDigits,
  formatCount,
  formatPercent,
  formatUsd,
  projectLine,
  shareTitle,
  subscriptionCaption,
  toolRowLabel,
  unrecognizedNote,
  valuePillText,
  type ShareOptions,
  type ShareSummary,
  type ShareToolName,
} from '../shareLogic'

const props = defineProps<{ summary: ShareSummary; options: ShareOptions }>()
const root = ref<SVGSVGElement | null>(null)
defineExpose({ root })

const WIDTH = 720
const HEIGHT = 1000
const X = 56
const RIGHT = 664
const INNER = RIGHT - X
const SANS = '"PingFang SC", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif'
const NUM = '"SF Pro Display", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif'
const svgStyle = {
  '--share-bg': '#141418',
  '--share-text': '#f5f6f8',
  '--share-muted': '#c8c9d1',
  '--share-faint': '#8b8c96',
  '--share-pill': '#1d3a28',
  '--share-pill-text': '#b7f5d0',
  '--tool-claude': '#e08a62',
  '--tool-codex': '#3cc3a6',
  '--share-track': '#2a2a30',
  'font-variant-numeric': 'tabular-nums',
}

function textWidth(text: string, size: number) {
  let width = 0
  for (const char of text) width += (char.codePointAt(0) ?? 0) > 255 ? size : size * 0.62
  return width
}

const title = computed(() => shareTitle(props.summary))
const caption = computed(() => subscriptionCaption(props.summary))
const digits = computed(() => formatAmountDigits(props.summary.month_estimated_cost_usd))
const pill = computed(() => valuePillText(props.summary.subscription_value_multiple))
const note = computed(() => unrecognizedNote(props.summary))
const requestsText = computed(() => `${formatCount(props.summary.requests)} 次`)
const cacheText = computed(() => formatPercent(props.summary.cache_hit_rate))
const projectText = computed(() => projectLine(props.summary.top_project, 24))

const layout = computed(() => {
  const slots: Array<'pill' | 'note' | 'bar' | 'row' | 'cache' | 'requests' | 'project' | 'footer'> = []
  if (pill.value) slots.push('pill')
  if (note.value) slots.push('note')
  slots.push('bar')
  const detailRows = props.options.toolDetails ? props.summary.tools : []
  detailRows.forEach(() => slots.push('row'))
  if (props.options.cacheHitRate) slots.push('cache')
  slots.push('requests')
  if (props.options.topProject) slots.push('project')
  slots.push('footer')
  const start = 372
  const end = 948
  const step = slots.length > 1 ? (end - start) / (slots.length - 1) : 0
  const yOf = (kind: typeof slots[number]) => {
    const index = slots.indexOf(kind)
    return start + step * index
  }
  const pillText = pill.value
  const pillBox = pillText
    ? { y: yOf('pill') - 28, width: Math.min(INNER, 58 + textWidth(pillText, 26) + 26) }
    : null
  let rowIndex = slots.indexOf('row')
  const rows = detailRows.map(row => {
    const placed = {
      y: start + step * rowIndex,
      tool: row.tool as ShareToolName,
      label: toolRowLabel(row),
      amount: formatUsd(row.estimated_cost_usd),
      icon: row.tool === 'Claude' ? claudeLogoUri : codexLogoUri,
    }
    rowIndex += 1
    return placed
  })
  let cursor = X
  const bars = costBarSegments(props.summary.tools, INNER).map(segment => {
    const bar = { ...segment, x: cursor }
    cursor += segment.width
    return bar
  })
  return {
    pillBox,
    noteY: note.value ? yOf('note') : null,
    barY: yOf('bar'),
    rows,
    cacheY: props.options.cacheHitRate ? yOf('cache') : null,
    requestsY: yOf('requests'),
    projectY: props.options.topProject ? yOf('project') : null,
    footerY: yOf('footer'),
    bars,
  }
})

function toolFill(tool: ShareToolName) {
  return tool === 'Claude' ? 'var(--tool-claude)' : 'var(--tool-codex)'
}
</script>

<template>
  <svg
    ref="root"
    class="share-card"
    xmlns="http://www.w3.org/2000/svg"
    xmlns:xlink="http://www.w3.org/1999/xlink"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    :width="WIDTH"
    :height="HEIGHT"
    role="img"
    :aria-label="title"
    :style="svgStyle"
  >
    <rect :width="WIDTH" :height="HEIGHT" rx="44" style="fill: var(--share-bg)" />
    <image :href="brandLogoUri" :xlink:href="brandLogoUri" :x="X" y="48" width="44" height="44" />
    <text :x="X + 60" y="70" dominant-baseline="middle" font-size="28" font-weight="600" :font-family="SANS" style="fill: var(--share-text)">{{ title }}</text>
    <text :x="X" y="168" font-size="26" :font-family="SANS" style="fill: var(--share-muted)">{{ caption }}</text>
    <text :x="X" y="300" font-weight="700" :font-family="NUM" style="fill: var(--share-text)">
      <tspan font-size="60">$</tspan><tspan font-size="108" dx="6">{{ digits }}</tspan>
    </text>
    <g v-if="layout.pillBox && pill">
      <rect :x="X" :y="layout.pillBox.y" :width="layout.pillBox.width" height="56" rx="28" style="fill: var(--share-pill)" />
      <g :transform="`translate(${X + 18} ${layout.pillBox.y + 16})`" fill="none" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" style="stroke: var(--share-pill-text)">
        <polyline points="22 7 13.5 15.5 8.5 10.5 2 17" />
        <polyline points="16 7 22 7 22 13" />
      </g>
      <text :x="X + 58" :y="layout.pillBox.y + 28" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="SANS" style="fill: var(--share-pill-text)">{{ pill }}</text>
    </g>
    <text v-if="layout.noteY != null && note" :x="X" :y="layout.noteY" dominant-baseline="middle" font-size="20" :font-family="SANS" style="fill: var(--share-faint)">{{ note }}</text>
    <defs>
      <clipPath id="share-card-bar">
        <rect :x="X" :y="layout.barY" :width="INNER" height="16" rx="8" />
      </clipPath>
    </defs>
    <rect :x="X" :y="layout.barY" :width="INNER" height="16" rx="8" style="fill: var(--share-track)" />
    <g clip-path="url(#share-card-bar)">
      <rect v-for="bar in layout.bars" :key="bar.tool" :x="bar.x" :y="layout.barY" :width="bar.width" height="16" :style="{ fill: toolFill(bar.tool) }" />
    </g>
    <g v-for="row in layout.rows" :key="row.tool">
      <image :href="row.icon" :xlink:href="row.icon" :x="X" :y="row.y - 18" width="36" height="36" />
      <text :x="X + 52" :y="row.y" dominant-baseline="middle" font-size="28" :font-family="SANS" style="fill: var(--share-text)">{{ row.label }}</text>
      <text :x="RIGHT" :y="row.y" text-anchor="end" dominant-baseline="middle" font-size="28" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ row.amount }}</text>
    </g>
    <text v-if="layout.cacheY != null" :x="X" :y="layout.cacheY" dominant-baseline="middle" font-size="26" :font-family="SANS" style="fill: var(--share-muted)">缓存命中率</text>
    <text v-if="layout.cacheY != null" :x="RIGHT" :y="layout.cacheY" text-anchor="end" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ cacheText }}</text>
    <text :x="X" :y="layout.requestsY" dominant-baseline="middle" font-size="26" :font-family="SANS" style="fill: var(--share-muted)">本月请求</text>
    <text :x="RIGHT" :y="layout.requestsY" text-anchor="end" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ requestsText }}</text>
    <text v-if="layout.projectY != null" :x="X" :y="layout.projectY" dominant-baseline="middle" font-size="24" :font-family="SANS" style="fill: var(--share-text)">{{ projectText }}</text>
    <text :x="X" :y="layout.footerY" font-size="20" :font-family="SANS" style="fill: var(--share-faint)">{{ SHARE_FOOTER }}</text>
  </svg>
</template>
