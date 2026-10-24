<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { CircleAlert, Coins, Compass, Database, ExternalLink, EyeOff, Folder, Gauge, History, Palette, Scale, ShieldCheck, SlidersHorizontal, Terminal, Trash2 } from 'lucide-vue-next'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import Toggle from '../components/Toggle.vue'
import {
  clearRecorderRecords,
  getEnvironment,
  getSettings,
  recorderInstall,
  recorderPlan,
  recorderPlanUninstall,
  recorderStatus,
  recorderUninstall,
  refreshQuotas,
  restoreAdvice,
  updateSettings,
  type AppSettings,
  type CredentialPreference,
  type EnvironmentInfo,
  type RecorderParts,
  type RecorderPlan,
  type RecorderStatus,
  type Theme,
  type Tool,
} from '../api'
import { adviceLabel, applyTheme, formatCalibration, formatPercent, parsePercent, privacyDetail, quotaMessageTone } from '../settingsLogic'
import { openOnboarding } from '../onboardingState'

type RecorderDialog = { kind: 'recorder'; enabling: boolean; parts: RecorderParts; plan: RecorderPlan }
type SettingsDialog = RecorderDialog | { kind: 'claude' } | { kind: 'clear' }

const settings = ref<AppSettings | null>(null)
const environment = ref<EnvironmentInfo | null>(null)
const recorder = ref<RecorderStatus | null>(null)
const loading = ref(true)
const loadError = ref('')
const notice = ref('')
const noticeError = ref(false)
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
const router = useRouter()
const parsedPercent = computed(() => parsePercent(percentText.value))
const locked = computed(() => confirming.value || dialog.value !== null)
const recorderDialog = computed(() => (dialog.value?.kind === 'recorder' ? dialog.value : null))
const privacyText = computed(() => privacyDetail(Boolean(settings.value?.claude_official_query), Boolean(settings.value?.codex_app_server)))

function showNotice(text: string, isError = false) {
  notice.value = text
  noticeError.value = isError
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

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const [nextSettings, nextEnvironment, nextRecorder] = await Promise.all([getSettings(), getEnvironment(), recorderStatus()])
    settings.value = nextSettings
    environment.value = nextEnvironment
    recorder.value = nextRecorder
    applyTheme(nextSettings.theme)
    if (nextSettings.calibration) {
      calibrationTool.value = nextSettings.calibration.tool
      percentText.value = formatPercent(nextSettings.calibration.five_hour_percent)
    }
  } catch (cause) {
    loadError.value = errorText(cause)
  } finally {
    loading.value = false
  }
}

async function save(next: AppSettings) {
  const saved = await updateSettings(next)
  settings.value = saved
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
    recorder.value = await recorderStatus()
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
    settings.value = await getSettings()
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
    recorder.value = await recorderStatus()
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

onMounted(load)
</script>

<template>
  <div class="dashboard-page settings-page">
    <header class="dashboard-header">
      <div>
        <h1>设置</h1>
        <p>管理数据来源、隐私与通用偏好</p>
      </div>
    </header>

    <div v-if="loading" class="center-state"><Database :size="28" /><strong>正在读取设置</strong></div>
    <div v-else-if="loadError" class="center-state">
      <CircleAlert :size="28" /><strong>读取设置失败</strong><p>{{ loadError }}</p><button type="button" @click="load">重试</button>
    </div>
    <template v-else-if="settings && environment && recorder">
      <p v-if="notice" class="settings-notice" :class="{ 'settings-notice-error': noticeError }" role="status">{{ notice }}</p>

      <section class="settings-banner">
        <span class="settings-banner-icon"><ShieldCheck :size="20" :stroke-width="2" aria-hidden="true" /></span>
        <div>
          <strong>所有数据均保留在本机</strong>
          <p>{{ privacyText }}</p>
        </div>
      </section>

      <article class="card settings-card">
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

        <div class="settings-row">
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

        <div class="settings-row">
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
        <header class="settings-card-head"><SlidersHorizontal :size="16" aria-hidden="true" />通用</header>

        <div class="settings-row">
          <span class="settings-icon"><Palette :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">外观</div>
            <p class="settings-desc">跟随系统，或固定为浅色、深色。</p>
          </div>
          <div class="settings-controls">
            <div class="segmented settings-theme" role="group" aria-label="外观">
              <button v-for="item in themes" :key="item.value" type="button" :class="{ active: settings.theme === item.value }" :aria-pressed="settings.theme === item.value" :disabled="locked" @click="setTheme(item.value)">{{ item.label }}</button>
            </div>
          </div>
        </div>

        <div class="settings-row">
          <span class="settings-icon"><Coins :size="18" aria-hidden="true" /></span>
          <div class="settings-main">
            <div class="settings-title">价格表</div>
            <p class="settings-desc">用于将 token 折算为 API 等价费用，随应用内置，当前版本 {{ environment.pricing_version }}；不联网更新。</p>
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
            <p class="settings-desc">再看一遍数据说明、已检测到的工具和额度来源。不会改动当前设置。</p>
          </div>
          <div class="settings-controls">
            <button type="button" class="settings-tonal" :disabled="locked" @click="openGuide">打开</button>
          </div>
        </div>
      </article>
    </template>

    <ConfirmDialog v-if="recorderDialog" :title="recorderDialog.enabling ? '安装记录器' : '卸载记录器'" :confirm-label="recorderDialog.enabling ? '安装' : '卸载'" :busy="confirming" @cancel="dialog = null" @confirm="confirmRecorder">
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
  </div>
</template>
