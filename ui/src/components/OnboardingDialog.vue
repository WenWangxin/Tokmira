<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { CircleCheck, Database, Info, ShieldCheck, Zap } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from './CodexLogo.vue'
import Toggle from './Toggle.vue'
import {
  detectTools,
  getSettings,
  recorderInstall,
  recorderPlan,
  recorderStatus,
  refreshQuotas,
  updateSettings,
  type DetectedTool,
} from '../api'
import {
  claudeQuotaHint,
  initialOnboardingChoices,
  installOutcome,
  onboardingChangeLines,
  outcomeTone,
  precisionPill,
  quotaOutcome,
  toolSubtitle,
  type OnboardingSelection,
} from '../onboardingLogic'
import { finishOnboarding } from '../onboardingState'

const SOURCES = [
  { source: 'ClaudeCli', name: 'Claude Code · 命令行' },
  { source: 'ClaudeDesktop', name: 'Claude · 桌面版' },
  { source: 'CodexCli', name: 'Codex · 命令行' },
  { source: 'CodexDesktop', name: 'Codex · 桌面版' },
] as const

const CHECKS = [
  '默认不发起任何网络请求',
  '绝不上传 Key、令牌或对话内容',
  '核心算法开源，结果可独立复现',
]

const step = ref(1)
const tools = ref<DetectedTool[] | null>(null)
const detecting = ref(true)
const detectError = ref('')
const choicesReady = ref(false)
const selection = ref<OnboardingSelection>({ statusline: true, claudeOfficial: false, codexOfficial: false })
const planError = ref('')
const feedback = ref<string[]>([])
const actionError = ref('')
const busy = ref(false)
const closing = ref(false)
const panel = ref<HTMLElement | null>(null)
const scroll = ref<HTMLElement | null>(null)
const footer = ref<HTMLElement | null>(null)
const direction = ref<'forward' | 'backward'>('forward')
const transitioning = ref(false)
let planGeneration = 0
let heightTimer = 0
let heightPanel: HTMLElement | null = null
let heightListener: ((event: TransitionEvent) => void) | null = null
let enterDone: (() => void) | null = null

const rows = computed(() => SOURCES.map(item => {
  const found = tools.value?.find(tool => tool.source === item.source)
  return {
    ...item,
    sessions: found?.sessions ?? 0,
    requests: found?.requests ?? 0,
    plan: found?.plan ?? null,
  }
}))
const changeLines = computed(() => onboardingChangeLines(selection.value))
const primaryLabel = computed(() => (step.value === 1 ? '开始' : step.value === 2 ? '下一步' : '完成'))
const nextDisabled = computed(() => busy.value || transitioning.value || (step.value === 2 && detecting.value && !tools.value))
const stepTitle = computed(() => {
  if (step.value === 1) return '你的数据，只留在这台电脑上'
  if (step.value === 2) return '已检测到以下工具'
  return '选择额度来源'
})

function errorText(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause)
}

async function loadTools() {
  detecting.value = true
  detectError.value = ''
  try {
    tools.value = await detectTools()
  } catch (cause) {
    tools.value = null
    detectError.value = errorText(cause)
  } finally {
    detecting.value = false
  }
}

async function loadChoices() {
  try {
    const [settings, recorder] = await Promise.all([getSettings(), recorderStatus()])
    selection.value = initialOnboardingChoices(
      settings.onboarding_completed,
      settings,
      recorder.statusline_installed || recorder.statusline_modified,
    )
  } catch {
    // Keep the first-run defaults. Saving later surfaces the read error.
  } finally {
    choicesReady.value = true
  }
}

async function refreshPlan() {
  const generation = ++planGeneration
  try {
    await recorderPlan({ statusline: true, hooks: false })
    if (generation === planGeneration) planError.value = ''
  } catch (cause) {
    if (generation === planGeneration) planError.value = errorText(cause)
  }
}

watch([step, () => selection.value.statusline, choicesReady], () => {
  if (!choicesReady.value || step.value !== 3 || !selection.value.statusline) {
    planGeneration += 1
    planError.value = ''
    return
  }
  void refreshPlan()
})

function onKey(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
}

function focusPrimary() {
  panel.value?.querySelector<HTMLButtonElement>('.onboarding-primary')?.focus()
}

function reducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function targetHeight(stepContent: HTMLElement) {
  if (!panel.value || !scroll.value || !footer.value) return 0
  const style = getComputedStyle(panel.value)
  const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
  return Math.min(
    Math.ceil(stepContent.offsetHeight + footer.value.offsetHeight + padding),
    window.innerHeight - 48,
  )
}

function finishHeightTransition() {
  window.clearTimeout(heightTimer)
  heightTimer = 0
  if (heightPanel && heightListener) heightPanel.removeEventListener('transitionend', heightListener)
  heightPanel = null
  heightListener = null
  const done = enterDone
  enterDone = null
  const wasTransitioning = transitioning.value
  if (panel.value) panel.value.style.height = ''
  transitioning.value = false
  done?.()
  if (wasTransitioning) void nextTick(focusPrimary)
}

function onStepEnter(el: Element, done: () => void) {
  enterDone = done
  const currentPanel = panel.value
  if (!(el instanceof HTMLElement) || !currentPanel || reducedMotion()) {
    finishHeightTransition()
    return
  }
  const height = targetHeight(el)
  if (!height) {
    finishHeightTransition()
    return
  }
  // Flush the starting pixel height before changing it to the measured target.
  void currentPanel.offsetHeight
  heightPanel = currentPanel
  heightListener = (event: TransitionEvent) => {
    if (event.target === currentPanel && event.propertyName === 'height') finishHeightTransition()
  }
  currentPanel.addEventListener('transitionend', heightListener)
  currentPanel.style.height = `${height}px`
  heightTimer = window.setTimeout(finishHeightTransition, 320)
}

function onStepAfterEnter() {
  finishHeightTransition()
}

function changeStep(delta: 1 | -1) {
  if (closing.value || busy.value || transitioning.value) return
  transitioning.value = true
  direction.value = delta > 0 ? 'forward' : 'backward'
  if (panel.value && !reducedMotion()) {
    panel.value.style.height = `${panel.value.getBoundingClientRect().height}px`
  }
  step.value += delta
}

async function next() {
  if (closing.value) return
  if (nextDisabled.value) return
  if (step.value < 3) {
    changeStep(1)
    return
  }
  await finish(false)
}

async function skip() {
  if (closing.value) return
  if (transitioning.value) return
  await finish(true)
}

async function finish(skipChanges: boolean) {
  if (closing.value || busy.value) return
  busy.value = true
  actionError.value = ''
  if (!skipChanges) feedback.value = []
  try {
    const current = await getSettings()
    if (!skipChanges) {
      if (selection.value.statusline) {
        const installed = await recorderInstall({ statusline: true, hooks: false })
        feedback.value = installOutcome(installed.backup_path, installed.notes)
      }
      const saved = await updateSettings({
        ...current,
        claude_official_query: selection.value.claudeOfficial,
        codex_app_server: selection.value.codexOfficial,
      })
      if (selection.value.claudeOfficial || selection.value.codexOfficial) {
        const refreshed = await refreshQuotas()
        feedback.value = [...feedback.value, ...quotaOutcome(selection.value, refreshed)]
      }
      await updateSettings({ ...saved, onboarding_completed: true })
      const message = feedback.value.join('；')
      await nextTick()
      finishOnboarding(message, outcomeTone(feedback.value))
      closing.value = true
      return
    }
    await updateSettings({ ...current, onboarding_completed: true })
    finishOnboarding('', 'success')
    closing.value = true
  } catch (cause) {
    actionError.value = errorText(cause)
    busy.value = false
  }
}

function toggle(key: keyof OnboardingSelection) {
  if (closing.value || busy.value) return
  selection.value = { ...selection.value, [key]: !selection.value[key] }
}

onMounted(() => {
  document.addEventListener('keydown', onKey, true)
  focusPrimary()
  void loadTools()
  void loadChoices()
})
onUnmounted(() => {
  planGeneration += 1
  finishHeightTransition()
  document.removeEventListener('keydown', onKey, true)
})
</script>

<template>
  <div class="onboarding-mask">
    <div
      ref="panel"
      class="onboarding-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="onboarding-title"
      :aria-describedby="step === 1 ? 'onboarding-lead' : undefined"
      :data-step="step"
      tabindex="-1"
    >
      <div ref="scroll" class="onboarding-scroll">
        <Transition :name="direction === 'forward' ? 'onboarding-step-forward' : 'onboarding-step-backward'" mode="out-in" @enter="onStepEnter" @after-enter="onStepAfterEnter" @enter-cancelled="finishHeightTransition">
          <div :key="step" class="onboarding-step">
            <template v-if="step === 1">
              <span class="onboarding-badge success"><ShieldCheck :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p id="onboarding-lead" class="onboarding-lead">token-audit 读取 Claude Code 与 Codex 存放在本机的日志，帮你看清额度余量、消耗去向与节省空间。</p>
              <ul class="onboarding-checks">
                <li v-for="item in CHECKS" :key="item"><CircleCheck :size="18" aria-hidden="true" />{{ item }}</li>
              </ul>
            </template>

            <template v-else-if="step === 2">
              <span class="onboarding-badge primary"><Database :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p v-if="detecting && !tools" class="onboarding-status">正在检测本机日志</p>
              <p v-else-if="detectError && !tools" class="onboarding-error">{{ detectError }} <button type="button" class="onboarding-text" @click="loadTools">重试</button></p>
              <ul v-else class="onboarding-tools">
                <li v-for="row in rows" :key="row.source" class="onboarding-tool">
                  <span class="onboarding-logo"><img v-if="row.source.startsWith('Claude')" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span>
                  <span class="onboarding-tool-main">
                    <strong>{{ row.name }}</strong>
                    <p>{{ toolSubtitle(row.source, row.sessions, row.plan) }}</p>
                    <p v-if="claudeQuotaHint(row.source, row.sessions)" class="onboarding-hint">{{ claudeQuotaHint(row.source, row.sessions) }}</p>
                  </span>
                  <span class="origin-pill settings-pill" :class="precisionPill(row.source, row.sessions).tone === 'success' ? 'pill-success' : 'pill-neutral'"><i />{{ precisionPill(row.source, row.sessions).text }}</span>
                </li>
              </ul>
              <p class="onboarding-footnote"><Info :size="15" aria-hidden="true" />网页与桌面端聊天不在统计范围内。</p>
            </template>

            <template v-else>
              <span class="onboarding-badge warning"><Zap :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p class="onboarding-lead">不开启任何来源也能使用：额度显示为“暂无数据”，用量与费用统计不受影响。</p>
              <div class="onboarding-choices">
                <div class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>Claude Code 状态栏记录（推荐，零联网）</strong>
                    <p>记录 Claude Code 传给状态栏的官方额度，零联网，仅在命令行版中生效。将写入 ~/.claude/settings.json 的 statusLine，原有状态栏保持显示。</p>
                  </div>
                  <Toggle :checked="selection.statusline" :disabled="busy" label="Claude Code 状态栏记录" @toggle="toggle('statusline')" />
                </div>
                <div class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>Claude 官方额度查询（需联网）</strong>
                    <p>读取本机 Claude 登录令牌，仅发送到 api.anthropic.com 查询额度；令牌只在内存中使用，不保存、不自行刷新。接口未公开，可能随时失效。</p>
                  </div>
                  <Toggle :checked="selection.claudeOfficial" :disabled="busy" label="Claude 官方额度查询" @toggle="toggle('claudeOfficial')" />
                </div>
                <div class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>Codex 官方额度（本地接口，需联网）</strong>
                    <p>启动本机的 codex app-server 读取 5 小时和每周额度，由 Codex 用你已登录的账号向 OpenAI 查询；token-audit 不读取你的令牌。</p>
                  </div>
                  <Toggle :checked="selection.codexOfficial" :disabled="busy" label="Codex 官方额度" @toggle="toggle('codexOfficial')" />
                </div>
              </div>
              <section class="onboarding-plan" aria-live="polite">
                <h3>将修改以下配置</h3>
                <ul>
                  <li v-for="(line, index) in changeLines" :key="`${index}-${line}`" :class="{ path: line.includes('settings.json') }">{{ line }}</li>
                </ul>
                <p v-for="(line, index) in feedback" :key="`done-${index}`" class="onboarding-outcome">{{ line }}</p>
                <p v-if="planError" class="onboarding-error">{{ planError }}</p>
                <p v-if="actionError" class="onboarding-error">{{ actionError }}</p>
              </section>
            </template>
          </div>
        </Transition>
      </div>

      <footer ref="footer" class="onboarding-footer">
        <div class="onboarding-steps" :aria-label="`第 ${step} 步，共 3 步`">
          <span v-for="index in 3" :key="index" :aria-current="index === step ? 'step' : undefined" />
        </div>
        <div class="onboarding-actions">
          <button v-if="step > 1" type="button" class="onboarding-text" :disabled="busy || transitioning" @click="changeStep(-1)">上一步</button>
          <button v-if="step === 3" type="button" class="onboarding-text" :disabled="busy || transitioning" @click="skip">暂不开启</button>
          <button type="button" class="onboarding-primary" :disabled="nextDisabled" @click="next">{{ primaryLabel }}</button>
        </div>
      </footer>
    </div>
  </div>
</template>
