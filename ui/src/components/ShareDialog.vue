<script setup lang="ts">
import { messages, m } from '../i18n/index.ts'
const t = messages
import { onMounted, onUnmounted, reactive, ref, unref } from 'vue'
import { Share, X } from 'lucide-vue-next'
import { getShareSummary, saveExport } from '../api'
import { showToast } from '../toast'
import ShareCard from './ShareCard.vue'
import Toggle from './Toggle.vue'
import {
  defaultShareOptions,
  exportErrorText,
  exportFileName,
  renderSharePng,
  shareMarkdown,
  type ShareSummary,
} from '../shareLogic'

const emit = defineEmits<{ close: [] }>()
const summary = ref<ShareSummary | null>(null)
const loading = ref(true)
const loadError = ref('')
const busy = ref(false)
const options = reactive(defaultShareOptions())
const card = ref<{ root: SVGSVGElement | null } | null>(null)
const panel = ref<HTMLElement | null>(null)
let alive = true

function onKey(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  emit('close')
}

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const value = await getShareSummary()
    if (alive) summary.value = value
  } catch (cause) {
    if (alive) loadError.value = exportErrorText(cause)
  } finally {
    if (alive) loading.value = false
  }
}

async function exportImage() {
  const svg = unref(card.value?.root ?? null)
  const data = summary.value
  if (!(svg instanceof SVGSVGElement) || !data || busy.value) return
  busy.value = true
  try {
    const png = await renderSharePng(svg)
    const bytes = new Uint8Array(await png.arrayBuffer())
    const saved = await saveExport(exportFileName(data, 'png'), bytes)
    if (saved) showToast(m().share.savedTo(saved), 'success', 5000)
  } catch (cause) {
    showToast(exportErrorText(cause), 'error')
  } finally {
    if (alive) busy.value = false
  }
}

async function exportMarkdown() {
  const data = summary.value
  if (!data || busy.value) return
  busy.value = true
  try {
    const bytes = new TextEncoder().encode(shareMarkdown(data, options))
    const saved = await saveExport(exportFileName(data, 'md'), bytes)
    if (saved) showToast(m().share.savedTo(saved), 'success', 5000)
  } catch (cause) {
    showToast(exportErrorText(cause), 'error')
  } finally {
    if (alive) busy.value = false
  }
}

onMounted(() => {
  document.addEventListener('keydown', onKey)
  panel.value?.focus()
  void load()
})
onUnmounted(() => {
  alive = false
  document.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div class="share-mask" @click.self="emit('close')">
    <div ref="panel" class="share-dialog" role="dialog" aria-modal="true" aria-labelledby="share-dialog-title" tabindex="-1">
      <button type="button" class="share-close" :aria-label="t.common.close" @click="emit('close')"><X :size="18" /></button>
      <div class="share-preview">
        <ShareCard v-if="summary" ref="card" :summary="summary" :options="options" />
        <p v-else-if="loading" class="share-preview-state">{{ t.share.loading }}</p>
        <p v-else class="share-preview-state">{{ loadError || t.share.loadFailed }}</p>
      </div>
      <div class="share-side">
        <div>
          <h2 id="share-dialog-title">{{ t.share.dialogTitle }}</h2>
          <p class="share-lead">{{ t.share.lead }}</p>
          <div class="share-options">
            <div class="share-option">
              <span>{{ t.share.mdTools }}</span>
              <Toggle :checked="options.toolDetails" :label="t.share.mdTools" @toggle="options.toolDetails = !options.toolDetails" />
            </div>
            <div class="share-option">
              <span>{{ t.share.cacheHitRate }}</span>
              <Toggle :checked="options.cacheHitRate" :label="t.share.cacheHitRate" @toggle="options.cacheHitRate = !options.cacheHitRate" />
            </div>
            <div class="share-option share-option-project">
              <div class="share-option">
                <span>{{ t.share.topProject }}</span>
                <Toggle :checked="options.topProject" :label="t.share.topProject" @toggle="options.topProject = !options.topProject" />
              </div>
              <p>{{ t.share.projectPrivacy }}</p>
            </div>
          </div>
        </div>
        <div class="share-actions">
          <button type="button" class="share-primary" :disabled="!summary || busy" @click="exportImage"><Share :size="18" />{{ t.share.exportImage }}</button>
          <button type="button" class="share-secondary" :disabled="!summary || busy" @click="exportMarkdown">{{ t.share.exportMarkdown }}</button>
        </div>
      </div>
    </div>
  </div>
</template>
