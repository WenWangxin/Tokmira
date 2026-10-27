<script setup lang="ts">
import { messages, m } from '../i18n/index.ts'
const t = messages
import { computed, ref } from 'vue'
import { TrendingUp } from 'lucide-vue-next'
import { brandLogoUri, claudeLogoUri, codexLogoUri } from '../shareLogos'
import {
  shareFooter,
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

const WIDTH = 800
const HEIGHT = 1000
const X = 52
const RIGHT = 748
const INNER = RIGHT - X
const SANS = '"PingFang SC", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif'
const NUM = '"SF Pro Display", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif'
const svgStyle = computed(() => {
  const styles = getComputedStyle(document.documentElement)
  const color = (name: string) => styles.getPropertyValue(name).trim()
  return {
    '--share-bg': color('--share-card-bg'),
    '--share-text': color('--share-card-text'),
    '--share-muted': color('--share-card-muted'),
    '--share-faint': color('--share-card-faint'),
    '--share-pill': color('--share-card-pill'),
    '--share-pill-text': color('--share-card-pill-text'),
    '--tool-claude': color('--share-card-claude'),
    '--tool-codex': color('--share-card-codex'),
    '--share-track': color('--share-card-track'),
    '--share-tile': color('--share-card-tile'),
    'font-variant-numeric': 'tabular-nums',
  }
})

function textWidth(text: string, size: number) {
  let width = 0
  for (const char of text) width += (char.codePointAt(0) ?? 0) > 255 ? size : size * 0.62
  const context = document.createElement('canvas').getContext('2d')
  if (context) {
    context.font = `${size}px ${SANS}`
    width = Math.max(width, context.measureText(text).width)
  }
  return width
}
const fitLength = (text: string, size: number, width: number) => textWidth(text, size) > width ? width : undefined

const title = computed(() => shareTitle(props.summary))
const caption = computed(() => subscriptionCaption(props.summary))
const digits = computed(() => formatAmountDigits(props.summary.month_estimated_cost_usd))
const pill = computed(() => valuePillText(props.summary.subscription_value_multiple))
const note = computed(() => unrecognizedNote(props.summary))
const requestsText = computed(() => t.value.share.requests(formatCount(props.summary.requests)))
const cacheText = computed(() => formatPercent(props.summary.cache_hit_rate))
const projectText = computed(() => projectLine(props.summary.top_project, 24))

const layout = computed(() => {
  const detailRows = props.options.toolDetails ? props.summary.tools : []
  const pillText = pill.value
  const pillBox = pillText
    ? { y: 386, width: Math.min(INNER, 58 + textWidth(pillText, 26) + 26) }
    : null
  const rows = detailRows.map((row, index) => {
    const placed = {
      y: 670 + index * 68,
      tool: row.tool as ShareToolName,
      label: toolRowLabel(row),
      amount: formatUsd(row.estimated_cost_usd),
      icon: row.tool === 'Claude' ? claudeLogoUri : codexLogoUri,
    }
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
    noteY: note.value ? 520 : null,
    barY: 604,
    rows,
    cacheY: props.options.cacheHitRate ? 670 + detailRows.length * 68 : null,
    requestsY: 670 + detailRows.length * 68 + (props.options.cacheHitRate ? 64 : 0),
    projectY: props.options.topProject ? 734 + detailRows.length * 68 + (props.options.cacheHitRate ? 64 : 0) : null,
    footerY: 964,
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
    <rect :width="WIDTH" :height="HEIGHT" rx="48" style="fill: var(--share-bg)" />
    <image :href="brandLogoUri" :xlink:href="brandLogoUri" :x="X" y="52" width="48" height="48" />
    <text :x="X + 64" y="76" dominant-baseline="middle" font-size="28" font-weight="500" :font-family="SANS" :textLength="fitLength(title, 28, INNER - 64)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-muted)">{{ title }}</text>
    <text :x="X" y="208" font-size="28" :font-family="SANS" :textLength="fitLength(caption, 28, INNER)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-muted)">{{ caption }}</text>
    <text :x="X" y="340" font-weight="700" :font-family="NUM" style="fill: var(--share-text)">
      <tspan font-size="60">$</tspan><tspan font-size="120" dx="6">{{ digits }}</tspan>
    </text>
    <g v-if="layout.pillBox && pill">
      <rect :x="X" :y="layout.pillBox.y" :width="layout.pillBox.width" height="72" rx="36" style="fill: var(--share-pill)" />
      <TrendingUp :x="X + 18" :y="layout.pillBox.y + 24" :size="24" :stroke-width="2.4" style="color: var(--share-pill-text)" />
      <text :x="X + 58" :y="layout.pillBox.y + 36" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="SANS" :textLength="fitLength(pill, 26, INNER - 84)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-pill-text)">{{ pill }}</text>
    </g>
    <text v-if="layout.noteY != null && note" :x="X" :y="layout.noteY" dominant-baseline="middle" font-size="20" :font-family="SANS" :textLength="fitLength(note, 20, INNER)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-faint)">{{ note }}</text>
    <defs>
      <clipPath id="share-card-bar">
        <rect :x="X" :y="layout.barY" :width="INNER" height="20" rx="10" />
      </clipPath>
    </defs>
    <rect :x="X" :y="layout.barY" :width="INNER" height="20" rx="10" style="fill: var(--share-track)" />
    <g clip-path="url(#share-card-bar)">
      <rect v-for="bar in layout.bars" :key="bar.tool" :x="bar.x" :y="layout.barY" :width="bar.width" height="20" :style="{ fill: toolFill(bar.tool) }" />
    </g>
    <g v-for="row in layout.rows" :key="row.tool">
      <rect :x="X" :y="row.y - 22" width="44" height="44" rx="14" style="fill: var(--share-tile)" />
      <image :href="row.icon" :xlink:href="row.icon" :x="X + 8" :y="row.y - 14" width="28" height="28" />
      <text :x="X + 60" :y="row.y" dominant-baseline="middle" font-size="28" :font-family="SANS" :textLength="fitLength(row.label, 28, INNER - 260)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-text)">{{ row.label }}</text>
      <text :x="RIGHT" :y="row.y" text-anchor="end" dominant-baseline="middle" font-size="28" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ row.amount }}</text>
    </g>
    <text v-if="layout.cacheY != null" :x="X" :y="layout.cacheY" dominant-baseline="middle" font-size="26" :font-family="SANS" style="fill: var(--share-muted)">{{ t.share.cacheHitRate }}</text>
    <text v-if="layout.cacheY != null" :x="RIGHT" :y="layout.cacheY" text-anchor="end" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ cacheText }}</text>
    <text :x="X" :y="layout.requestsY" dominant-baseline="middle" font-size="26" :font-family="SANS" style="fill: var(--share-muted)">{{ t.share.monthRequests }}</text>
    <text :x="RIGHT" :y="layout.requestsY" text-anchor="end" dominant-baseline="middle" font-size="26" font-weight="600" :font-family="NUM" style="fill: var(--share-text)">{{ requestsText }}</text>
    <text v-if="layout.projectY != null" :x="X" :y="layout.projectY" dominant-baseline="middle" font-size="24" :font-family="SANS" :textLength="fitLength(projectText, 24, INNER)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-text)">{{ projectText }}</text>
    <text :x="X" :y="layout.footerY" font-size="20" :font-family="SANS" :textLength="fitLength(shareFooter(), 20, INNER)" lengthAdjust="spacingAndGlyphs" style="fill: var(--share-faint)">{{ shareFooter() }}</text>
  </svg>
</template>
