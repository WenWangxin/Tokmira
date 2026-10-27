<script setup lang="ts">
import { showToast } from '../toast'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import { BellRing, CircleAlert, Coins, Compass, Database, ExternalLink, EyeOff, Folder, Gauge, History, Info, Languages, LogIn, MonitorOff, Palette, Power, RefreshCw, Scale, SlidersHorizontal, Tag, Terminal, Trash2 } from 'lucide-vue-next'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import Toggle from '../components/Toggle.vue'
import { vSelectionSlide } from '../components/selectionSlide'
import {
  checkUpdate,
  clearRecorderRecords,
  getAutostart,
  getUpdateStatus,
  installUpdate,
  getEnvironment,
  getSettings,
  notificationStatus,
  openLoginItemsSettings,
  recorderInstall,
  recorderPlan,
  recorderPlanUninstall,
  recorderStatus,
  recorderUninstall,
  refreshQuotas,
  restartApp,
  restoreAdvice,
  setAutostart,
  updateSettings,
  type AppSettings,
  type AutostartState,
  type CredentialPreference,
  type EnvironmentInfo,
  type MenuBarQuota,
  type Language,
  type QuotaOutcome,
  type NotificationStatus,
  type RecorderParts,
  type RecorderPlan,
  type RecorderStatus,
  type Theme,
  type Tool,
  type UpdateProgress,
  type UpdateStatus,
} from '../api'
import { adviceLabel, applyTheme, formatCalibration, formatPercent, parsePercent, quotaMessageTone, quotaMessage } from '../settingsLogic'
import { applyLanguage, m, messages } from '../i18n/index.ts'
import { notificationLine, trayPlace } from '../onboardingLogic'
import { lastCheckedLine, progressLine, updateActionLabel, type UpdateStage } from '../updateLogic'
import { openOnboarding } from '../onboardingState'
import { settingsRevision } from '../realtime'
import PageSkeleton from '../components/PageSkeleton.vue'
import { cacheVersion, cacheVersionMatches, getPageCache, invalidatePageCache, pageCacheKey, setPageCache } from '../pageCache'

type RecorderDialog = { kind: 'recorder'; enabling: boolean; parts: RecorderParts; plan: RecorderPlan }
type SettingsDialog = RecorderDialog | { kind: 'claude' } | { kind: 'clear' }

type SettingsSnapshot = { settings: AppSettings; environment: EnvironmentInfo; recorder: RecorderStatus }
const settingsKey = pageCacheKey('settings')
const initial = getPageCache<SettingsSnapshot>(settingsKey)
const settings = ref<AppSettings | null>(initial?.settings ?? null)
const environment = ref<EnvironmentInfo | null>(initial?.environment ?? null)
const recorder = ref<RecorderStatus | null>(initial?.recorder ?? null)
const pending = ref(initial === undefined)
const loading = ref(false)
const reveal = ref(initial === undefined)
let loadingTimer: ReturnType<typeof setTimeout> | undefined
const loadError = ref('')
const dialog = ref<SettingsDialog | null>(null)
const dialogError = ref('')
const confirming = ref(false)
const claudeResult = ref<QuotaOutcome | null>(null)
const t = messages
const calibrationTool = ref<Tool>('Claude')
const percentText = ref('')

const themes = computed((): { value: Theme; label: string }[] => [
  { value: 'system', label: t.value.settings.themeSystem },
  { value: 'light', label: t.value.settings.themeLight },
  { value: 'dark', label: t.value.settings.themeDark },
])
const languages = computed((): { value: Language; label: string }[] => [
  { value: 'system', label: t.value.settings.languageSystem },
  { value: 'zh', label: '中文' },
  { value: 'en', label: 'English' },
])
const credentials = computed((): { value: CredentialPreference; label: string }[] => [
  { value: 'Auto', label: m().settings.credentialAuto },
  { value: 'ClaudeDesktop', label: m().settings.credentialDesktop },
  { value: 'ClaudeCode', label: 'Claude Code' },
])
const quotaModes: { value: MenuBarQuota; label: string }[] = [
  { value: 'off', label: '只显示图标' },
  { value: 'claude', label: 'Claude' },
  { value: 'codex', label: 'Codex' },
  { value: 'both', label: '两者都显示' },
]
const router = useRouter()
const parsedPercent = computed(() => parsePercent(percentText.value))
const locked = computed(() => confirming.value || dialog.value !== null)
const recorderDialog = computed(() => (dialog.value?.kind === 'recorder' ? dialog.value : null))

function showNotice(text: string, isError = false) {
  showToast(text, isError ? 'error' : 'success')
}
function errorText(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause)
}
function recorderPill(installed: boolean, modified: boolean) {
  if (modified) return { text: m().settings.pillModified, tone: 'pill-warning' }
  if (installed) return { text: m().settings.pillInstalled, tone: 'pill-success' }
  return { text: m().settings.pillNotInstalled, tone: 'pill-neutral' }
}
function networkPill(enabled: boolean) {
  return enabled ? { text: m().settings.pillOn, tone: 'pill-success' } : { text: m().settings.pillNeedsNetwork, tone: 'pill-neutral' }
}
function partEnabled(part: 'statusline' | 'hooks') {
  if (!recorder.value) return false
  return part === 'statusline'
    ? recorder.value.statusline_installed || recorder.value.statusline_modified
    : recorder.value.hooks_installed || recorder.value.hooks_modified
}
function calibrationLine() {
  const calibration = settings.value?.calibration
  if (!calibration) return ''
  const date = new Date(calibration.recorded_at)
  if (Number.isNaN(date.getTime())) return m().settings.calibrationLine(calibration.tool, formatPercent(calibration.five_hour_percent), '')
  return formatCalibration(calibration.tool, calibration.five_hour_percent, date)
}
function claudeCopy(preference: CredentialPreference) {
  const sent = m().settings.claudeSent
  if (preference === 'ClaudeDesktop') return `${m().settings.claudeCopyDesktop}${sent}`
  if (preference === 'ClaudeCode') return `${m().settings.claudeCopyCode}${sent}`
  return `${m().settings.claudeCopyAuto}${sent}`
}

function saveSnapshot() {
  if (settings.value && environment.value && recorder.value) {
    setPageCache(settingsKey, { settings: settings.value, environment: environment.value, recorder: recorder.value })
  }
}

// System state, read from the OS rather than settings.json.
const userAgent = navigator.userAgent
const isMac = /Mac/.test(userAgent)
const notifications = ref<NotificationStatus>('unknown')
const notifyLine = computed(() => notificationLine(notifications.value))
const autostart = ref<AutostartState | null>(null)
const systemBusy = ref(false)

async function loadSystemState() {
  const [status, login] = await Promise.allSettled([notificationStatus(), getAutostart()])
  notifications.value = status.status === 'fulfilled' ? status.value : 'unknown'
  if (login.status === 'fulfilled') autostart.value = login.value
}

async function toggleAutostart() {
  if (!autostart.value || systemBusy.value) return
  systemBusy.value = true
  try {
    autostart.value = await setAutostart(!autostart.value.enabled)
  } catch (cause) {
    showNotice(errorText(cause), true)
  } finally {
    systemBusy.value = false
  }
}

async function setBackgroundOption(change: Partial<Pick<AppSettings, 'silent_start' | 'menu_bar_quota'>>) {
  if (!settings.value || locked.value) return
  try { await save({ ...settings.value, ...change }) }
  catch (cause) { showNotice(errorText(cause), true) }
}

// Updates: checked only when the user clicks, or by the opt-in daily background check.
const update = ref<UpdateStatus | null>(null)
const checking = ref(false)
const installing = ref(false)
const updateOpen = ref(false)
const progress = ref<UpdateProgress | null>(null)
const updateError = ref('')
const updateStage = computed<UpdateStage>(() => installing.value ? 'installing' : update.value?.installed ? 'installed' : 'ready')
let stopProgress: Promise<() => void> | undefined

async function loadUpdate() {
  try { update.value = await getUpdateStatus() } catch { /* the version row stays as it was */ }
}

function openUpdate() {
  updateError.value = ''
  updateOpen.value = true
}

async function onCheckUpdate() {
  if (checking.value || installing.value) return
  checking.value = true
  try {
    update.value = await checkUpdate()
    if (update.value.available) openUpdate()
    else showToast(m().settings.upToDate(update.value.current_version), 'success')
  } catch (cause) {
    showNotice(errorText(cause), true)
  } finally {
    checking.value = false
  }
}

async function onUpdateConfirm() {
  if (update.value?.installed) {
    await restartApp()
    return
  }
  installing.value = true
  updateError.value = ''
  progress.value = null
  try {
    await installUpdate()
    update.value = await getUpdateStatus()
  } catch (cause) {
    updateError.value = errorText(cause)
  } finally {
    installing.value = false
  }
}

function closeUpdate() {
  if (!installing.value) updateOpen.value = false
}

async function setAutoCheck(enabled: boolean) {
  if (!settings.value || locked.value) return
  try { await save({ ...settings.value, auto_check_updates: enabled }) }
  catch (cause) { showNotice(errorText(cause), true) }
}

watch(settingsRevision, () => { void load(); void loadSystemState(); void loadUpdate() })

/** Back from System Settings: show what changed there. */
function onWindowFocus() {
  void loadSystemState()
}

async function load() {
  if (!settings.value) {
    pending.value = true
    loadingTimer = setTimeout(() => { loading.value = true }, 250)
  }
  loadError.value = ''
  const version = cacheVersion(settingsKey)
  try {
    const [nextSettings, nextEnvironment, nextRecorder] = await Promise.all([getSettings(), getEnvironment(), recorderStatus()])
    if (!cacheVersionMatches(settingsKey, version)) return
    settings.value = nextSettings
    environment.value = nextEnvironment
    recorder.value = nextRecorder
    saveSnapshot()
    applyTheme(nextSettings.theme)
    applyLanguage(nextSettings.language)
    if (nextSettings.calibration) {
      calibrationTool.value = nextSettings.calibration.tool
      percentText.value = formatPercent(nextSettings.calibration.five_hour_percent)
    }
  } catch (cause) {
    if (!settings.value) loadError.value = errorText(cause)
  } finally {
    if (loadingTimer) clearTimeout(loadingTimer)
    pending.value = false
    loading.value = false
  }
}

async function save(next: AppSettings) {
  const previous = settings.value
  const saved = await updateSettings(next)
  settings.value = saved
  saveSnapshot()
  if (previous && (saved.claude_official_query !== previous.claude_official_query || saved.codex_app_server !== previous.codex_app_server || JSON.stringify(saved.calibration) !== JSON.stringify(previous.calibration) || saved.claude_credential !== previous.claude_credential)) {
    invalidatePageCache('overview', 'advice', 'usageWindows', 'usageDetail', 'sessions', 'sessionDetail')
  }
  applyTheme(saved.theme)
  applyLanguage(saved.language)
  return saved
}

async function setLanguage(language: Language) {
  if (!settings.value || settings.value.language === language || locked.value) return
  try {
    await save({ ...settings.value, language })
    claudeResult.value = null
    await load()
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function setTheme(theme: Theme) {
  if (!settings.value || settings.value.theme === theme || locked.value) return
  const previous = settings.value.theme
  applyTheme(theme)
  try {
    await save({ ...settings.value, theme })
  } catch (cause) {
    applyTheme(previous)
    showNotice(errorText(cause), true)
  }
}

async function setAdviceNotifications(enabled: boolean) {
  if (!settings.value || locked.value) return
  try {
    await save({ ...settings.value, advice_notifications: enabled })
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function setCodex(enabled: boolean) {
  if (!settings.value) return
  try {
    await save({ ...settings.value, codex_app_server: enabled })
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function setCredential(value: CredentialPreference) {
  if (!settings.value || settings.value.claude_credential === value) return
  try {
    await save({ ...settings.value, claude_credential: value })
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

function onCredentialChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  if (value === 'Auto' || value === 'ClaudeDesktop' || value === 'ClaudeCode') void setCredential(value)
}

async function onClaudeToggle() {
  if (!settings.value || locked.value) return
  if (settings.value.claude_official_query) {
    try {
      await save({ ...settings.value, claude_official_query: false })
      claudeResult.value = null
    } catch (cause) {
      showNotice(errorText(cause), true)
    }
    return
  }
  dialogError.value = ''
  dialog.value = { kind: 'claude' }
}

async function confirmClaude() {
  if (!settings.value || confirming.value) return
  confirming.value = true
  dialogError.value = ''
  try {
    await save({ ...settings.value, claude_official_query: true })
    claudeResult.value = (await refreshQuotas()).claude
    dialog.value = null
  } catch (cause) {
    dialogError.value = errorText(cause)
  } finally {
    confirming.value = false
  }
}

async function onRecorderToggle(part: 'statusline' | 'hooks') {
  if (!recorder.value || locked.value) return
  const enabling = !partEnabled(part)
  const parts = { statusline: part === 'statusline', hooks: part === 'hooks' }
  confirming.value = true
  try {
    const plan = enabling ? await recorderPlan(parts) : await recorderPlanUninstall(parts)
    dialogError.value = ''
    dialog.value = { kind: 'recorder', enabling, parts, plan }
  } catch (cause) {
    showNotice(errorText(cause), true)
  } finally {
    confirming.value = false
  }
}

async function confirmRecorder() {
  const current = recorderDialog.value
  if (!current || confirming.value) return
  confirming.value = true
  dialogError.value = ''
  try {
    const result = current.enabling ? await recorderInstall(current.parts) : await recorderUninstall(current.parts)
    invalidatePageCache('overview', 'advice', 'usageWindows', 'usageDetail', 'sessions', 'sessionDetail')
    recorder.value = await recorderStatus()
    saveSnapshot()
    if (result.backup_path) showNotice(m().settings.backedUp(result.backup_path))
    else if (result.notes.length) showNotice(result.notes.map(note => m().recorder.note[note]).join(' '))
    else showNotice(m().settings.noFileChanged)
    dialog.value = null
  } catch (cause) {
    dialogError.value = errorText(cause)
  } finally {
    confirming.value = false
  }
}

async function calibrate() {
  if (!settings.value || parsedPercent.value === null || locked.value) return
  try {
    await save({
      ...settings.value,
      calibration: { tool: calibrationTool.value, five_hour_percent: parsedPercent.value, recorded_at: new Date().toISOString() },
    })
    showNotice(m().settings.calibrated)
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function clearCalibration() {
  if (!settings.value || locked.value) return
  try {
    await save({ ...settings.value, calibration: null })
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function restore(key: string) {
  if (locked.value) return
  try {
    await restoreAdvice(key)
    invalidatePageCache('advice', 'sessionDetail')
    settings.value = await getSettings()
    saveSnapshot()
  } catch (cause) {
    showNotice(errorText(cause), true)
  }
}

async function confirmClear() {
  if (confirming.value) return
  confirming.value = true
  dialogError.value = ''
  try {
    await clearRecorderRecords()
    invalidatePageCache('overview', 'advice', 'usageWindows', 'usageDetail', 'sessions', 'sessionDetail')
    recorder.value = await recorderStatus()
    saveSnapshot()
    showNotice(m().settings.cleared)
    dialog.value = null
  } catch (cause) {
    dialogError.value = errorText(cause)
  } finally {
    confirming.value = false
  }
}

function openClear() {
  if (locked.value) return
  dialogError.value = ''
  dialog.value = { kind: 'clear' }
}

function openGuide() {
  if (locked.value) return
  openOnboarding()
  if (router.currentRoute.value.path !== '/') void router.push('/')
}

onMounted(() => {
  void load()
  void loadSystemState()
  void loadUpdate()
  window.addEventListener('focus', onWindowFocus)
  if ('__TAURI_INTERNALS__' in window) stopProgress = listen<UpdateProgress>('update-progress', event => { progress.value = event.payload })
})
onUnmounted(() => {
  if (loadingTimer) clearTimeout(loadingTimer)
  window.removeEventListener('focus', onWindowFocus)
  void stopProgress?.then(stop => stop())
})
</script>

<template>
  <div class="dashboard-page settings-page" :class="{ 'no-entry-reveal': !reveal }">
    <header class="dashboard-header">
      <div>
        <h1>{{ t.pages.settings.title }}</h1>
        <p>{{ t.pages.settings.subtitle }}</p>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="settings" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="loadError" class="center-state">
      <CircleAlert :size="28" /><strong>{{ t.settings.loadFailed }}</strong><p>{{ loadError }}</p><button type="button" @click="load">{{ t.common.retry }}</button>
    </div>
    <template v-else-if="settings && environment && recorder">
      <article class="card settings-card content-reveal">
        <header class="settings-card-head"><Database :size="16" aria-hidden="true" />{{ t.settings.dataSources }}</header>

        <div class="settings-row">
          <span class="settings-icon"><Gauge :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.statuslineTitle }}<span class="origin-pill settings-pill" :class="recorderPill(recorder.statusline_installed, recorder.statusline_modified).tone"><i />{{ recorderPill(recorder.statusline_installed, recorder.statusline_modified).text }}</span></div>
            <p class="settings-desc">{{ t.settings.statuslineDesc }}</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="partEnabled('statusline')" :disabled="locked" :label="t.settings.statuslineToggle" @toggle="onRecorderToggle('statusline')" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><History :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">Claude Code · hooks<span class="origin-pill settings-pill" :class="recorderPill(recorder.hooks_installed, recorder.hooks_modified).tone"><i />{{ recorderPill(recorder.hooks_installed, recorder.hooks_modified).text }}</span></div>
            <p class="settings-desc">{{ t.settings.hooksDesc }}</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="partEnabled('hooks')" :disabled="locked" label="Claude Code hooks" @toggle="onRecorderToggle('hooks')" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Terminal :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.codexTitle }}<span class="origin-pill settings-pill" :class="networkPill(settings.codex_app_server).tone"><i />{{ networkPill(settings.codex_app_server).text }}</span></div>
            <p class="settings-desc">{{ t.settings.codexDesc }}</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.codex_app_server" :disabled="locked" :label="t.settings.codexToggle" @toggle="setCodex(!settings.codex_app_server)" />
          </div>
        </div>

        <div class="settings-row settings-row-complex">
          <span class="settings-icon"><ExternalLink :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.claudeTitle }}<span class="origin-pill settings-pill" :class="networkPill(settings.claude_official_query).tone"><i />{{ networkPill(settings.claude_official_query).text }}</span></div>
            <p class="settings-desc">{{ t.settings.claudeDesc }}</p>
            <p v-if="claudeResult" class="settings-result" :class="quotaMessageTone(claudeResult)" role="status">{{ quotaMessage(claudeResult) }}</p>
          </div>
          <div class="settings-controls">
            <label class="settings-select-wrap">
              <select class="settings-select" :value="settings.claude_credential" :disabled="locked" :aria-label="t.settings.credentialSource" @change="onCredentialChange">
                <option v-for="item in credentials" :key="item.value" :value="item.value">{{ item.label }}</option>
              </select>
            </label>
            <Toggle :checked="settings.claude_official_query" :disabled="locked" :label="t.settings.claudeTitle" @toggle="onClaudeToggle" />
          </div>
        </div>

        <div class="settings-row settings-row-complex">
          <span class="settings-icon"><Scale :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.calibrationTitle }}</div>
            <p class="settings-desc">{{ t.settings.calibrationDesc }}</p>
            <p v-if="settings.calibration" class="settings-calibration">{{ calibrationLine() }}<button type="button" class="settings-text" :disabled="locked" @click="clearCalibration">{{ t.common.clear }}</button></p>
          </div>
          <div class="settings-controls">
            <label class="settings-select-wrap">
              <select v-model="calibrationTool" class="settings-select" :disabled="locked" :aria-label="t.settings.calibrationTool">
                <option value="Claude">Claude</option>
                <option value="Codex">Codex</option>
              </select>
            </label>
            <label class="settings-percent">
              <input v-model="percentText" inputmode="decimal" :aria-label="t.settings.calibrationPercent" :disabled="locked" />
              <span>%</span>
            </label>
            <button type="button" class="settings-tonal" :disabled="locked || parsedPercent === null" @click="calibrate">{{ t.settings.calibrate }}</button>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><Power :size="16" aria-hidden="true" />{{ t.settings.background }}</header>

        <div class="settings-row">
          <span class="settings-icon"><BellRing :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.alertsTitle }}</div>
            <p class="settings-desc">{{ t.settings.alertsDesc }}</p>
            <p class="settings-note settings-notify">
              <span class="settings-notify-status" :class="`notify-${notifyLine.tone}`"><i aria-hidden="true" />{{ notifyLine.text }}</span>
            </p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.advice_notifications" :disabled="locked" :label="t.settings.alertsTitle" @toggle="setAdviceNotifications(!settings.advice_notifications)" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><LogIn :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.autostartTitle }}</div>
            <p class="settings-desc">{{ t.settings.autostartDesc }}</p>
            <p v-if="autostart && !autostart.available" class="settings-note">{{ t.settings.autostartDev }}</p>
            <p v-if="autostart?.requires_approval" class="settings-note">{{ t.settings.autostartApproval }} <button type="button" class="settings-link" @click="openLoginItemsSettings">{{ t.settings.openLoginItems }}</button></p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="Boolean(autostart?.enabled)" :disabled="locked || systemBusy || !autostart?.available" :label="t.settings.autostartTitle" @toggle="toggleAutostart" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><MonitorOff :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.silentTitle }}</div>
            <p class="settings-desc">{{ t.settings.silentDesc(trayPlace(userAgent)) }}</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.silent_start" :disabled="locked" :label="t.settings.silentTitle" @toggle="setBackgroundOption({ silent_start: !settings.silent_start })" />
          </div>
        </div>

        <div v-if="isMac" class="settings-row">
          <span class="settings-icon"><Gauge :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">菜单栏显示额度</div>
            <p class="settings-desc">选择图标旁显示的 5 小时额度。</p>
          </div>
          <div class="settings-controls">
            <div v-selection-slide class="segmented settings-theme" role="group" aria-label="菜单栏显示额度">
              <button v-for="item in quotaModes" :key="item.value" type="button" :class="{ active: settings.menu_bar_quota === item.value }" :aria-pressed="settings.menu_bar_quota === item.value" :disabled="locked" @click="setBackgroundOption({ menu_bar_quota: item.value })">{{ item.label }}</button>
            </div>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><SlidersHorizontal :size="16" aria-hidden="true" />{{ t.settings.general }}</header>

        <div class="settings-row">
          <span class="settings-icon"><Palette :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.appearance }}</div>
            <p class="settings-desc">{{ t.settings.appearanceDesc }}</p>
          </div>
          <div class="settings-controls">
            <div v-selection-slide class="segmented settings-theme" role="group" :aria-label="t.settings.appearance">
              <button v-for="item in themes" :key="item.value" type="button" :class="{ active: settings.theme === item.value }" :aria-pressed="settings.theme === item.value" :disabled="locked" @click="setTheme(item.value)">{{ item.label }}</button>
            </div>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Languages :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.language }}</div>
            <p class="settings-desc">{{ t.settings.languageDesc }}</p>
          </div>
          <div class="settings-controls">
            <div v-selection-slide class="segmented settings-theme" role="group" :aria-label="t.settings.language">
              <button v-for="item in languages" :key="item.value" type="button" :class="{ active: settings.language === item.value }" :aria-pressed="settings.language === item.value" :disabled="locked" @click="setLanguage(item.value)">{{ item.label }}</button>
            </div>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Coins :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.pricingTitle }}</div>
            <p class="settings-desc">{{ t.settings.pricingDesc(environment.pricing_version) }}</p>
            <p v-if="environment.pricing_note" class="settings-note">{{ environment.pricing_note }}</p>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Folder :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.logsTitle }}</div>
            <ul class="settings-paths">
              <li><span>{{ t.settings.claudeLogs }}</span><code>{{ environment.claude_logs.path }}</code><em :class="{ missing: !environment.claude_logs.exists }">{{ environment.claude_logs.exists ? t.settings.exists : t.settings.missing }}</em></li>
              <li><span>{{ t.settings.codexLogs }}</span><code>{{ environment.codex_logs.path }}</code><em :class="{ missing: !environment.codex_logs.exists }">{{ environment.codex_logs.exists ? t.settings.exists : t.settings.missing }}</em></li>
              <li><span>{{ t.settings.appData }}</span><code>{{ environment.data_dir }}</code></li>
            </ul>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><EyeOff :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.hiddenAdvice }}</div>
            <p v-if="!settings.dismissed_advice.length" class="settings-desc">{{ t.settings.none }}</p>
            <ul v-else class="settings-list">
              <li v-for="key in settings.dismissed_advice" :key="key">
                <span>{{ adviceLabel(key) }}</span>
                <button type="button" class="settings-text" :disabled="locked" @click="restore(key)">{{ t.common.restore }}</button>
              </li>
            </ul>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Trash2 :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.clearTitle }}</div>
            <p class="settings-desc">{{ t.settings.clearDesc }}</p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-danger" :disabled="locked" @click="openClear">{{ t.common.clear }}</button>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Compass :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.guideTitle }}</div>
            <p class="settings-desc">{{ t.settings.guideDesc }}</p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-tonal" :disabled="locked" @click="openGuide">{{ t.common.open }}</button>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><Info :size="16" aria-hidden="true" />{{ t.settings.about }}</header>

        <div class="settings-row">
          <span class="settings-icon"><Tag :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.currentVersion(update?.current_version ?? '—') }}</div>
            <p class="settings-desc">{{ lastCheckedLine(update?.last_checked_at ?? null) }}</p>
            <p v-if="update?.available" class="settings-note">{{ t.settings.newVersion(update.available.version) }} <button type="button" class="settings-link" @click="openUpdate">{{ t.settings.viewAndInstall }}</button></p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-tonal" :disabled="checking || installing" @click="onCheckUpdate">{{ checking ? t.settings.checking : t.settings.checkUpdate }}</button>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><RefreshCw :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">{{ t.settings.autoCheckTitle }}</div>
            <p class="settings-desc">{{ t.settings.autoCheckDesc }}</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.auto_check_updates" :disabled="locked" :label="t.settings.autoCheckTitle" @toggle="setAutoCheck(!settings.auto_check_updates)" />
          </div>
        </div>
      </article>
    </template>

    <Transition name="dialog"><ConfirmDialog v-if="recorderDialog" :title="recorderDialog.enabling ? t.settings.installRecorder : t.settings.uninstallRecorder" :confirm-label="recorderDialog.enabling ? t.settings.install : t.settings.uninstall" :busy="confirming" @cancel="dialog = null" @confirm="confirmRecorder">
      <p v-if="!recorderDialog.plan.changes.length" class="confirm-copy">{{ t.settings.noChanges }}</p>
      <ul v-else class="plan-list">
        <li v-for="(change, index) in recorderDialog.plan.changes" :key="`${change.key_path}-${index}`" class="plan-change">
          <strong>{{ change.file }}</strong>
          <span>{{ change.key_path }} · {{ t.recorder.action[change.action] }}</span>
          <span v-if="change.before_command">{{ t.settings.before }}<code>{{ change.before_command }}</code></span>
          <span v-if="change.after_command">{{ t.settings.after }}<code>{{ change.after_command }}</code></span>
        </li>
      </ul>
      <p v-for="note in recorderDialog.plan.notes" :key="note" class="confirm-copy">{{ t.recorder.note[note] }}</p>
      <p v-if="recorderDialog.plan.backup_path" class="confirm-copy">{{ t.settings.backupBefore(recorderDialog.plan.backup_path) }}</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="dialog?.kind === 'claude' && settings" :title="t.settings.enableClaude" :confirm-label="t.settings.enableAndQuery" :busy="confirming" @cancel="dialog = null" @confirm="confirmClaude">
      <p class="confirm-copy">{{ claudeCopy(settings.claude_credential) }}</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="dialog?.kind === 'clear'" :title="t.settings.clearTitle" :confirm-label="t.common.clear" danger :busy="confirming" @cancel="dialog = null" @confirm="confirmClear">
      <p class="confirm-copy">{{ t.settings.clearConfirm }}</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="updateOpen && update?.available" :title="t.settings.updateTitle(update.available.version)" :confirm-label="updateActionLabel(updateStage)" :busy="installing" @cancel="closeUpdate" @confirm="onUpdateConfirm">
      <p class="confirm-copy update-notes">{{ update.available.notes ?? t.settings.noNotes }}</p>
      <p v-if="installing" class="confirm-copy">{{ progress ? progressLine(progress.downloaded, progress.total) : t.settings.connecting }}</p>
      <p v-if="update.installed" class="confirm-copy">{{ t.settings.installedRestart }}</p>
      <p v-if="updateError" class="settings-result bad">{{ updateError }}</p>
    </ConfirmDialog></Transition>
  </div>
</template>
