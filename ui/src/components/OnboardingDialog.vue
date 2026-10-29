<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { BellRing, Bot, CircleCheck, Database, Info, LoaderCircle, ShieldCheck, Zap } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from './CodexLogo.vue'
import Toggle from './Toggle.vue'
import {
  detectTools,
  getAutostart,
  getSettings,
  notificationStatus,
  requestNotifications,
  setAutostart,
  recorderInstall,
  recorderPlan,
  recorderStatus,
  refreshQuotas,
  updateSettings,
  type DetectedTool,
  type NotificationStatus,
} from '../api'
import {
  quotaSources,
  detectedToolRows,
  initialOnboardingChoices,
  installOutcome,
  notificationLine,
  onboardingSummary,
  quotaOutcome,
  trayPlace,
  type OnboardingSelection,
  type QuotaLine,
} from '../onboardingLogic'
import { m, messages } from '../i18n/index.ts'
import { finishOnboarding } from '../onboardingState'

const t = messages
const checks = computed(() => t.value.onboarding.checks)
const quotaChoices = computed(() => quotaSources())

const STEPS = 4
const REMINDER_STEP = 3
const step = ref(1)
const tools = ref<DetectedTool[] | null>(null)
const detecting = ref(true)
const detectError = ref('')
const choicesReady = ref(false)
const selection = ref<OnboardingSelection>({ statusline: true, claudeOfficial: false, codexOfficial: false })
// Background reminders: applied on 完成 and on 暂不开启 alike, which only skips quota sources.
const reminders = ref({ notifications: true, autostart: true })
const autostartAvailable = ref(true)
const notifications = ref<NotificationStatus>('unknown')
const notificationBusy = ref(false)
let askedForNotifications = false
const place = computed(() => trayPlace(navigator.userAgent))
const planError = ref('')
const feedback = ref<string[]>([])
const quotaFeedback = ref<QuotaLine[]>([])
const operationErrors = ref<string[]>([])
const actionError = ref('')
const busy = ref(false)
// Set only while “完成” installs and queries, so the button can show it is working.
const finishing = ref(false)
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

const rows = computed(() => detectedToolRows(tools.value ?? []))
const notifyLine = computed(() => notificationLine(notifications.value))
const primaryLabel = computed(() => (step.value === 1 ? m().onboarding.start : step.value < STEPS ? m().onboarding.next : m().onboarding.finish))
const nextDisabled = computed(() => busy.value || transitioning.value || (step.value === 2 && detecting.value && !tools.value))
const stepTitle = computed(() => {
  if (step.value === 1) return m().onboarding.step1Title
  if (step.value === 2) return m().onboarding.step2Title
  if (step.value === REMINDER_STEP) return m().onboarding.step3Title
  return m().onboarding.step4Title
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
    const [settings, recorder, autostart] = await Promise.all([getSettings(), recorderStatus(), getAutostart()])
    selection.value = initialOnboardingChoices(
      settings.onboarding_completed,
      settings,
      recorder.statusline_installed || recorder.statusline_modified,
    )
    autostartAvailable.value = autostart.available
    reminders.value = settings.onboarding_completed
      ? { notifications: settings.advice_notifications, autostart: autostart.enabled }
      : { notifications: true, autostart: autostart.available }
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

async function loadNotifications() {
  try {
    notifications.value = await notificationStatus()
  } catch {
    notifications.value = 'unknown'
  }
}

/** Shows the system prompt; macOS asks only once, later answers live in System Settings. */
async function askForNotifications() {
  if (notificationBusy.value) return
  notificationBusy.value = true
  try {
    notifications.value = await requestNotifications()
  } catch {
    notifications.value = 'unknown'
  } finally {
    notificationBusy.value = false
  }
}

// Ask as soon as the reminder step opens, so the system prompt sits next to its explanation.
watch([step, notifications, () => reminders.value.notifications], () => {
  if (step.value !== REMINDER_STEP || askedForNotifications || !reminders.value.notifications) return
  if (notifications.value !== 'not_determined') return
  askedForNotifications = true
  void askForNotifications()
})

/** Coming back from System Settings: show what the user changed there. */
function onWindowFocus() {
  if (step.value === REMINDER_STEP) void loadNotifications()
}

/** Saves the reminder choices; a failed login item is reported, not fatal. */
async function applyReminders(current: Awaited<ReturnType<typeof getSettings>>) {
  const saved = await updateSettings({ ...current, advice_notifications: reminders.value.notifications, silent_start: reminders.value.autostart && autostartAvailable.value ? true : current.silent_start })
  if (!autostartAvailable.value) return saved
  try {
    const autostart = await getAutostart()
    if (autostart.enabled !== reminders.value.autostart) await setAutostart(reminders.value.autostart)
  } catch (cause) {
    const line = m().onboarding.autostartFailed(errorText(cause))
    feedback.value = [...feedback.value, line]
    operationErrors.value = [...operationErrors.value, line]
  }
  return saved
}

watch([step, () => selection.value.statusline, choicesReady], () => {
  if (!choicesReady.value || step.value !== STEPS || !selection.value.statusline) {
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
  if (step.value < STEPS) {
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
  finishing.value = !skipChanges
  actionError.value = ''
  if (!skipChanges) { feedback.value = []; quotaFeedback.value = []; operationErrors.value = [] }
  try {
    const current = await applyReminders(await getSettings())
    if (!skipChanges) {
      if (selection.value.statusline) {
        const installed = await recorderInstall({ statusline: true, hooks: false })
        feedback.value = [...feedback.value, ...installOutcome(installed.backup_path, installed.notes)]
      }
      const saved = await updateSettings({
        ...current,
        claude_official_query: selection.value.claudeOfficial,
        codex_app_server: selection.value.codexOfficial,
      })
      if (selection.value.claudeOfficial || selection.value.codexOfficial) {
        const refreshed = await refreshQuotas()
        quotaFeedback.value = quotaOutcome(selection.value, refreshed)
      }
      await updateSettings({ ...saved, onboarding_completed: true })
      const summary = onboardingSummary(quotaFeedback.value, operationErrors.value)
      await nextTick()
      finishOnboarding(summary.text, summary.tone)
      closing.value = true
      return
    }
    await updateSettings({ ...current, onboarding_completed: true })
    const summary = operationErrors.value.length || quotaFeedback.value.length ? onboardingSummary(quotaFeedback.value, operationErrors.value) : { text: '', tone: 'success' as const }
    finishOnboarding(summary.text, summary.tone)
    closing.value = true
  } catch (cause) {
    actionError.value = errorText(cause)
    busy.value = false
    finishing.value = false
  }
}

function toggle(key: keyof OnboardingSelection) {
  if (closing.value || busy.value) return
  selection.value = { ...selection.value, [key]: !selection.value[key] }
}

function toggleReminder(key: 'notifications' | 'autostart') {
  if (closing.value || busy.value) return
  reminders.value = { ...reminders.value, [key]: !reminders.value[key] }
}

onMounted(() => {
  document.addEventListener('keydown', onKey, true)
  window.addEventListener('focus', onWindowFocus)
  focusPrimary()
  void loadTools()
  void loadChoices()
  void loadNotifications()
})
onUnmounted(() => {
  planGeneration += 1
  finishHeightTransition()
  document.removeEventListener('keydown', onKey, true)
  window.removeEventListener('focus', onWindowFocus)
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
              <p id="onboarding-lead" class="onboarding-lead">{{ t.onboarding.lead }}</p>
              <ul class="onboarding-checks">
                <li v-for="item in checks" :key="item"><CircleCheck :size="18" aria-hidden="true" />{{ item }}</li>
              </ul>
            </template>

            <template v-else-if="step === 2">
              <span class="onboarding-badge primary"><Database :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p v-if="detecting && !tools" class="onboarding-status">{{ t.onboarding.detecting }}</p>
              <p v-else-if="detectError && !tools" class="onboarding-error">{{ detectError }} <button type="button" class="onboarding-text" @click="loadTools">{{ t.common.retry }}</button></p>
              <ul v-else class="onboarding-tools">
                <li v-for="row in rows" :key="row.source" class="onboarding-tool">
                  <span class="onboarding-logo"><img v-if="row.logo === 'claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else-if="row.logo === 'codex'" /><Bot v-else :size="18" aria-hidden="true" /></span>
                  <span class="onboarding-tool-main">
                    <strong>{{ row.name }}</strong>
                    <p>{{ row.subtitle }}</p>
                    <p v-if="row.hint" class="onboarding-hint">{{ row.hint }}</p>
                  </span>
                  <span class="origin-pill settings-pill" :class="row.pill.tone === 'success' ? 'pill-success' : 'pill-neutral'"><i />{{ row.pill.text }}</span>
                </li>
              </ul>
              <p class="onboarding-footnote"><Info :size="15" aria-hidden="true" />{{ t.onboarding.chatsExcluded }}</p>
            </template>

            <template v-else-if="step === REMINDER_STEP">
              <span class="onboarding-badge primary"><BellRing :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p class="onboarding-lead">{{ t.onboarding.remindersLead(place) }}</p>
              <div class="onboarding-choices">
                <div class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>{{ t.onboarding.alertsTitle }}</strong>
                    <p>{{ t.onboarding.alertsDesc }}</p>
                  </div>
                  <Toggle :checked="reminders.notifications" :disabled="busy" :label="t.onboarding.alertsTitle" @toggle="toggleReminder('notifications')" />
                </div>
                <div class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>{{ t.onboarding.autostartTitle }}</strong>
                    <p>{{ t.onboarding.autostartDesc(place) }}</p>
                    <p v-if="!autostartAvailable" class="onboarding-hint">{{ t.onboarding.autostartDev }}</p>
                  </div>
                  <Toggle :checked="reminders.autostart && autostartAvailable" :disabled="busy || !autostartAvailable" :label="t.onboarding.autostartTitle" @toggle="toggleReminder('autostart')" />
                </div>
              </div>
              <section class="onboarding-plan onboarding-notify" aria-live="polite">
                <h3>{{ t.onboarding.systemNotifications }}</h3>
                <div class="onboarding-notify-row">
                  <span class="onboarding-notify-status" :class="`notify-${notifyLine.tone}`"><i aria-hidden="true" />{{ notifyLine.text }}</span>
                </div>
              </section>
            </template>

            <template v-else>
              <span class="onboarding-badge warning"><Zap :size="22" :stroke-width="2" aria-hidden="true" /></span>
              <h2 id="onboarding-title">{{ stepTitle }}</h2>
              <p class="onboarding-lead">{{ t.onboarding.sourcesLead }}</p>
              <div class="onboarding-choices">
                <div v-for="item in quotaChoices" :key="item.key" class="onboarding-choice">
                  <div class="onboarding-choice-main">
                    <strong>{{ item.title }}</strong>
                    <p>{{ item.description }}</p>
                    <p v-if="item.hint && selection[item.key]" class="onboarding-hint">{{ item.hint }}</p>
                  </div>
                  <Toggle :checked="selection[item.key]" :disabled="busy" :label="item.title" @toggle="toggle(item.key)" />
                </div>
              </div>
              <section v-if="feedback.length || quotaFeedback.length || planError || actionError" class="onboarding-plan" aria-live="polite">
                <p v-for="(line, index) in feedback" :key="`done-${index}`" class="onboarding-outcome">{{ line }}</p>
                <p v-for="(line, index) in quotaFeedback" :key="`quota-${index}`" class="onboarding-outcome">{{ line.text }}</p>
                <p v-if="planError" class="onboarding-error">{{ planError }}</p>
                <p v-if="actionError" class="onboarding-error">{{ actionError }}</p>
              </section>
            </template>
          </div>
        </Transition>
      </div>

      <footer ref="footer" class="onboarding-footer">
        <div class="onboarding-steps" :aria-label="t.onboarding.stepOf(step, STEPS)">
          <span v-for="index in STEPS" :key="index" :aria-current="index === step ? 'step' : undefined" />
        </div>
        <div class="onboarding-actions">
          <button v-if="step > 1" type="button" class="onboarding-text" :disabled="busy || transitioning" @click="changeStep(-1)">{{ t.onboarding.back }}</button>
          <button v-if="step === STEPS" type="button" class="onboarding-text" :disabled="busy || transitioning" @click="skip">{{ t.onboarding.skip }}</button>
          <button type="button" class="onboarding-primary" :class="{ 'onboarding-primary-wide': step === STEPS }" :disabled="nextDisabled" :aria-busy="finishing" @click="next">
            <template v-if="finishing"><LoaderCircle class="onboarding-spinner" :size="16" aria-hidden="true" />{{ t.onboarding.enabling }}</template>
            <template v-else>{{ primaryLabel }}</template>
          </button>
        </div>
      </footer>
    </div>
  </div>
</template>
