<script setup lang="ts">
import { showToast } from '../toast'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import { BellRing, CircleAlert, Coins, Compass, Database, ExternalLink, EyeOff, Folder, Gauge, History, Info, LogIn, MonitorOff, Palette, Power, RefreshCw, Scale, SlidersHorizontal, Tag, Terminal, Trash2 } from 'lucide-vue-next'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import Toggle from '../components/Toggle.vue'
import CodexLogo from '../components/CodexLogo.vue'
import ClaudeLogo from '../assets/logos/claude.svg'
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
  type NotificationStatus,
  type RecorderParts,
  type RecorderPlan,
  type RecorderStatus,
  type Theme,
  type Tool,
  type UpdateProgress,
  type UpdateStatus,
} from '../api'
import { adviceLabel, applyTheme, formatCalibration, formatPercent, parsePercent, quotaMessageTone } from '../settingsLogic'
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
const claudeResult = ref('')
const calibrationTool = ref<Tool>('Claude')
const percentText = ref('')

const themes: { value: Theme; label: string }[] = [
  { value: 'system', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
]
const credentials: { value: CredentialPreference; label: string }[] = [
  { value: 'Auto', label: '自动' },
  { value: 'ClaudeDesktop', label: 'Claude 桌面版' },
  { value: 'ClaudeCode', label: 'Claude Code' },
]
const menuBarTools: Tool[] = ['Claude', 'Codex']
/** The menu bar has room for two quotas, stacked. */
const MENU_BAR_TOOLS_MAX = 2
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
  if (modified) return { text: '配置已被修改', tone: 'pill-warning' }
  if (installed) return { text: '已安装', tone: 'pill-success' }
  return { text: '未安装', tone: 'pill-neutral' }
}
function networkPill(enabled: boolean) {
  return enabled ? { text: '已开启', tone: 'pill-success' } : { text: '需联网', tone: 'pill-neutral' }
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
  if (Number.isNaN(date.getTime())) return `上次校准：${calibration.tool} ${formatPercent(calibration.five_hour_percent)}%`
  return formatCalibration(calibration.tool, calibration.five_hour_percent, date)
}
function claudeCopy(preference: CredentialPreference) {
  const sent = '查询发往 https://api.anthropic.com/api/oauth/usage，只发送登录令牌以查询额度，不包含对话内容。令牌只在内存中使用，不保存、不自行刷新。'
  if (preference === 'ClaudeDesktop') return `将读取 Claude 桌面版保存在本机的登录令牌。macOS 会弹出钥匙串授权框，请选择“允许”。${sent}`
  if (preference === 'ClaudeCode') return `将读取 Claude Code 保存在本机的登录令牌。${sent}`
  return `将按自动顺序读取凭据：优先 Claude 桌面版，找不到或已过期时改用 Claude Code。读取桌面版时，macOS 会弹出钥匙串授权框，请选择“允许”。${sent}`
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

async function setBackgroundOption(change: Partial<Pick<AppSettings, 'silent_start' | 'menu_bar_tools'>>) {
  if (!settings.value || locked.value) return
  try { await save({ ...settings.value, ...change }) }
  catch (cause) { showNotice(errorText(cause), true) }
}

function menuBarShown(tool: Tool) {
  return Boolean(settings.value?.menu_bar_tools.includes(tool))
}
function menuBarFull(tool: Tool) {
  return !menuBarShown(tool) && (settings.value?.menu_bar_tools.length ?? 0) >= MENU_BAR_TOOLS_MAX
}
function toggleMenuBarTool(tool: Tool) {
  if (!settings.value || menuBarFull(tool)) return
  const current = settings.value.menu_bar_tools
  const next = current.includes(tool) ? current.filter((item) => item !== tool) : [...current, tool]
  void setBackgroundOption({ menu_bar_tools: next })
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
    else showToast(`已是最新版本 ${update.value.current_version}`, 'success')
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
  return saved
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
      claudeResult.value = ''
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
    if (result.backup_path) showNotice(`已备份到 ${result.backup_path}`)
    else if (result.notes.length) showNotice(result.notes.join(' '))
    else showNotice('没有修改配置文件')
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
    showNotice('已记录手动校准')
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
    showNotice('已清除本地记录')
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
        <h1>设置</h1>
        <p>管理数据来源、隐私与通用偏好</p>
      </div>
    </header>

    <PageSkeleton v-if="pending && loading" kind="settings" />
    <div v-else-if="pending" class="page-await" />
    <div v-else-if="loadError" class="center-state">
      <CircleAlert :size="28" /><strong>读取设置失败</strong><p>{{ loadError }}</p><button type="button" @click="load">重试</button>
    </div>
    <template v-else-if="settings && environment && recorder">
      <article class="card settings-card content-reveal">
        <header class="settings-card-head"><Database :size="16" aria-hidden="true" />数据来源</header>

        <div class="settings-row">
          <span class="settings-icon"><Gauge :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">Claude Code · 状态栏记录<span class="origin-pill settings-pill" :class="recorderPill(recorder.statusline_installed, recorder.statusline_modified).tone"><i />{{ recorderPill(recorder.statusline_installed, recorder.statusline_modified).text }}</span></div>
            <p class="settings-desc">记录 Claude Code 传给状态栏的官方额度，零联网，仅在命令行版中生效。将写入 ~/.claude/settings.json 的 statusLine，原有状态栏保持显示。</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="partEnabled('statusline')" :disabled="locked" label="Claude Code 状态栏记录" @toggle="onRecorderToggle('statusline')" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><History :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">Claude Code · hooks<span class="origin-pill settings-pill" :class="recorderPill(recorder.hooks_installed, recorder.hooks_modified).tone"><i />{{ recorderPill(recorder.hooks_installed, recorder.hooks_modified).text }}</span></div>
            <p class="settings-desc">在提交提示与回答结束时记录时间和会话 ID，不记录提示内容。日志已含时间信息，一般无需开启。</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="partEnabled('hooks')" :disabled="locked" label="Claude Code hooks" @toggle="onRecorderToggle('hooks')" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Terminal :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">Codex 官方额度（本地接口）<span class="origin-pill settings-pill" :class="networkPill(settings.codex_app_server).tone"><i />{{ networkPill(settings.codex_app_server).text }}</span></div>
            <p class="settings-desc">启动本机的 codex app-server 读取 5 小时和每周额度，由 Codex 用你已登录的账号向 OpenAI 查询；token-audit 不读取你的令牌。</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.codex_app_server" :disabled="locked" label="Codex 官方额度" @toggle="setCodex(!settings.codex_app_server)" />
          </div>
        </div>

        <div class="settings-row settings-row-complex">
          <span class="settings-icon"><ExternalLink :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">Claude 官方额度查询<span class="origin-pill settings-pill" :class="networkPill(settings.claude_official_query).tone"><i />{{ networkPill(settings.claude_official_query).text }}</span></div>
            <p class="settings-desc">读取本机 Claude 登录令牌，仅发送到 api.anthropic.com 查询额度；令牌只在内存中使用，不保存、不自行刷新。接口未公开，可能随时失效。</p>
            <p v-if="claudeResult" class="settings-result" :class="quotaMessageTone(claudeResult)" role="status">{{ claudeResult }}</p>
          </div>
          <div class="settings-controls">
            <label class="settings-select-wrap">
              <select class="settings-select" :value="settings.claude_credential" :disabled="locked" aria-label="Claude 凭据来源" @change="onCredentialChange">
                <option v-for="item in credentials" :key="item.value" :value="item.value">{{ item.label }}</option>
              </select>
            </label>
            <Toggle :checked="settings.claude_official_query" :disabled="locked" label="Claude 官方额度查询" @toggle="onClaudeToggle" />
          </div>
        </div>

        <div class="settings-row settings-row-complex">
          <span class="settings-icon"><Scale :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">手动校准</div>
            <p class="settings-desc">未开启额度来源时可用：在 Claude Code 中执行 /usage，把显示的 5 小时百分比填在这里。</p>
            <p v-if="settings.calibration" class="settings-calibration">{{ calibrationLine() }}<button type="button" class="settings-text" :disabled="locked" @click="clearCalibration">清除</button></p>
          </div>
          <div class="settings-controls">
            <label class="settings-select-wrap">
              <select v-model="calibrationTool" class="settings-select" :disabled="locked" aria-label="校准工具">
                <option value="Claude">Claude</option>
                <option value="Codex">Codex</option>
              </select>
            </label>
            <label class="settings-percent">
              <input v-model="percentText" inputmode="decimal" aria-label="5 小时已用百分比" :disabled="locked" />
              <span>%</span>
            </label>
            <button type="button" class="settings-tonal" :disabled="locked || parsedPercent === null" @click="calibrate">校准</button>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><Power :size="16" aria-hidden="true" />后台运行</header>

        <div class="settings-row">
          <span class="settings-icon"><BellRing :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">省额度提醒</div>
            <p class="settings-desc">关闭窗口后仍在后台运行，发现上下文过长、缓存过期重建等浪费额度的情况时发系统通知。只在本机判断，同一问题只提醒一次。</p>
            <p class="settings-note settings-notify">
              <span class="settings-notify-status" :class="`notify-${notifyLine.tone}`"><i aria-hidden="true" />{{ notifyLine.text }}</span>
            </p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.advice_notifications" :disabled="locked" label="省额度提醒" @toggle="setAdviceNotifications(!settings.advice_notifications)" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><LogIn :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">开机自动启动</div>
            <p class="settings-desc">登录后自动启动 token-audit。</p>
            <p v-if="autostart && !autostart.available" class="settings-note">开发版不可用，请在打包后的应用里设置。</p>
            <p v-if="autostart?.requires_approval" class="settings-note">需要在系统设置中允许 <button type="button" class="settings-link" @click="openLoginItemsSettings">打开登录项设置</button></p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="Boolean(autostart?.enabled)" :disabled="locked || systemBusy || !autostart?.available" label="开机自动启动" @toggle="toggleAutostart" />
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><MonitorOff :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">静默启动</div>
            <p class="settings-desc">启动时不打开窗口，只在{{ trayPlace(userAgent) }}显示图标。</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.silent_start" :disabled="locked" label="静默启动" @toggle="setBackgroundOption({ silent_start: !settings.silent_start })" />
          </div>
        </div>

        <div v-if="isMac" class="settings-row">
          <span class="settings-icon"><Gauge :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">菜单栏显示额度</div>
            <p class="settings-desc">在图标旁显示 5 小时额度，最多选 {{ MENU_BAR_TOOLS_MAX }} 个，选两个时上下排列；都不选只显示图标。</p>
          </div>
          <div class="settings-controls">
            <div class="segmented settings-menubar-tools" role="group" aria-label="菜单栏显示额度">
              <button v-for="item in menuBarTools" :key="item" type="button" :class="{ active: menuBarShown(item) }" :aria-pressed="menuBarShown(item)" :disabled="locked || menuBarFull(item)" :title="menuBarFull(item) ? `最多同时显示 ${MENU_BAR_TOOLS_MAX} 个` : undefined" @click="toggleMenuBarTool(item)">
                <img v-if="item === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else />{{ item }}
              </button>
            </div>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><SlidersHorizontal :size="16" aria-hidden="true" />通用</header>

        <div class="settings-row">
          <span class="settings-icon"><Palette :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">外观</div>
            <p class="settings-desc">跟随系统，或固定为浅色、深色。</p>
          </div>
          <div class="settings-controls">
            <div v-selection-slide class="segmented settings-theme" role="group" aria-label="外观">
              <button v-for="item in themes" :key="item.value" type="button" :class="{ active: settings.theme === item.value }" :aria-pressed="settings.theme === item.value" :disabled="locked" @click="setTheme(item.value)">{{ item.label }}</button>
            </div>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Coins :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">价格表</div>
            <p class="settings-desc">将 token 折算为 API 等价费用。随应用内置，当前版本 {{ environment.pricing_version }}，不联网更新。</p>
            <p v-if="environment.pricing_note" class="settings-note">{{ environment.pricing_note }}</p>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Folder :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">日志位置</div>
            <ul class="settings-paths">
              <li><span>Claude 日志</span><code>{{ environment.claude_logs.path }}</code><em :class="{ missing: !environment.claude_logs.exists }">{{ environment.claude_logs.exists ? '存在' : '未找到' }}</em></li>
              <li><span>Codex 日志</span><code>{{ environment.codex_logs.path }}</code><em :class="{ missing: !environment.codex_logs.exists }">{{ environment.codex_logs.exists ? '存在' : '未找到' }}</em></li>
              <li><span>应用数据</span><code>{{ environment.data_dir }}</code></li>
            </ul>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><EyeOff :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">已隐藏的建议</div>
            <p v-if="!settings.dismissed_advice.length" class="settings-desc">无</p>
            <ul v-else class="settings-list">
              <li v-for="key in settings.dismissed_advice" :key="key">
                <span>{{ adviceLabel(key) }}</span>
                <button type="button" class="settings-text" :disabled="locked" @click="restore(key)">恢复</button>
              </li>
            </ul>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Trash2 :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">清除记录</div>
            <p class="settings-desc">删除状态栏和 hooks 的本地记录（不影响 Claude Code / Codex 自己的日志）</p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-danger" :disabled="locked" @click="openClear">清除</button>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Compass :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">重新查看引导</div>
            <p class="settings-desc">再看一遍数据说明、已检测到的工具、后台提醒和额度来源。不会改动当前设置。</p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-tonal" :disabled="locked" @click="openGuide">打开</button>
          </div>
        </div>
      </article>

      <article class="card settings-card">
        <header class="settings-card-head"><Info :size="16" aria-hidden="true" />关于与更新</header>

        <div class="settings-row">
          <span class="settings-icon"><Tag :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">当前版本 {{ update?.current_version ?? '—' }}</div>
            <p class="settings-desc">{{ lastCheckedLine(update?.last_checked_at ?? null) }}</p>
            <p v-if="update?.available" class="settings-note">发现新版本 {{ update.available.version }} <button type="button" class="settings-link" @click="openUpdate">查看并安装</button></p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-tonal" :disabled="checking || installing" @click="onCheckUpdate">{{ checking ? '检查中…' : '检查更新' }}</button>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><RefreshCw :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">自动检查更新</div>
            <p class="settings-desc">每天最多检查一次，有新版本时提醒。只连接 GitHub 获取更新清单，不发送版本号、设备信息或任何使用数据。</p>
          </div>
          <div class="settings-controls">
            <Toggle :checked="settings.auto_check_updates" :disabled="locked" label="自动检查更新" @toggle="setAutoCheck(!settings.auto_check_updates)" />
          </div>
        </div>
      </article>
    </template>

    <Transition name="dialog"><ConfirmDialog v-if="recorderDialog" :title="recorderDialog.enabling ? '安装记录器' : '卸载记录器'" :confirm-label="recorderDialog.enabling ? '安装' : '卸载'" :busy="confirming" @cancel="dialog = null" @confirm="confirmRecorder">
      <p v-if="!recorderDialog.plan.changes.length" class="confirm-copy">没有需要修改的配置项。</p>
      <ul v-else class="plan-list">
        <li v-for="(change, index) in recorderDialog.plan.changes" :key="`${change.key_path}-${index}`" class="plan-change">
          <strong>{{ change.file }}</strong>
          <span>{{ change.key_path }} · {{ change.action }}</span>
          <span v-if="change.before_command">改前：<code>{{ change.before_command }}</code></span>
          <span v-if="change.after_command">改后：<code>{{ change.after_command }}</code></span>
        </li>
      </ul>
      <p v-for="note in recorderDialog.plan.notes" :key="note" class="confirm-copy">{{ note }}</p>
      <p v-if="recorderDialog.plan.backup_path" class="confirm-copy">修改前会完整备份到 {{ recorderDialog.plan.backup_path }}</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="dialog?.kind === 'claude' && settings" title="开启 Claude 官方额度查询" confirm-label="开启并查询" :busy="confirming" @cancel="dialog = null" @confirm="confirmClaude">
      <p class="confirm-copy">{{ claudeCopy(settings.claude_credential) }}</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="dialog?.kind === 'clear'" title="清除记录" confirm-label="清除" danger :busy="confirming" @cancel="dialog = null" @confirm="confirmClear">
      <p class="confirm-copy">删除状态栏和 hooks 的本地记录。Claude Code 与 Codex 自己的日志不会被删除。</p>
      <p v-if="dialogError" class="settings-result bad">{{ dialogError }}</p>
    </ConfirmDialog>

    <ConfirmDialog v-else-if="updateOpen && update?.available" :title="`新版本 ${update.available.version}`" :confirm-label="updateActionLabel(updateStage)" :busy="installing" @cancel="closeUpdate" @confirm="onUpdateConfirm">
      <p class="confirm-copy update-notes">{{ update.available.notes ?? '这个版本没有附更新说明。' }}</p>
      <p v-if="installing" class="confirm-copy">{{ progress ? progressLine(progress.downloaded, progress.total) : '正在连接 GitHub…' }}</p>
      <p v-if="update.installed" class="confirm-copy">新版本已安装，重启 token-audit 后生效。</p>
      <p v-if="updateError" class="settings-result bad">{{ updateError }}</p>
    </ConfirmDialog></Transition>
  </div>
</template>
