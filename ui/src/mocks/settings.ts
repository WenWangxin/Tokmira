import type { AppSettings, EnvironmentInfo, RecorderParts, RecorderPlan, RecorderStatus } from '../api'

/** In-memory browser preview. Nothing here is read from the real home directory. */
export const mockSettings: AppSettings = {
  onboarding_completed: true,
  claude_official_query: false,
  claude_credential: 'Auto',
  codex_app_server: false,
  theme: 'system',
  language: 'system',
  calibration: {
    tool: 'Claude',
    five_hour_percent: 28,
    recorded_at: '2026-10-24T06:30:00.000Z',
  },
  dismissed_advice: ['claude:long_context'],
  advice_notifications: true,
  tray_hint_shown: true,
  silent_start: false,
  menu_bar_tools: [],
  auto_check_updates: false,
  update_last_checked_at: null,
  update_notified_version: null,
}

export const mockEnvironment: EnvironmentInfo = {
  claude_logs: { path: '/Users/preview/.claude/projects', exists: true },
  codex_logs: { path: '/Users/preview/.codex/sessions', exists: false },
  data_dir: '/Users/preview/Library/Application Support/token-audit',
  pricing_version: '2026-10-24',
  pricing_note: '仅供估算，非实际账单；claude-haiku-5-5 仅按 10 万 token 以内价格计算，不计长上下文加价。',
  advice_version: '2026-10-24',
  backup_dir: '/Users/preview/Library/Application Support/token-audit/recorder/backups',
}

export const mockRecorderStatus: RecorderStatus = {
  statusline_installed: true,
  statusline_modified: false,
  hooks_installed: true,
  hooks_modified: false,
  statusline_records: 12,
  hook_events: 4,
  last_statusline_record: '2026-10-24T06:00:00.000Z',
  last_hook_event: '2026-10-24T06:10:00.000Z',
}

const previewFile = '/Users/preview/.claude/settings.json'
const hookCommand = '"/usr/local/bin/token-audit" hook event --tool claude'
const statuslineCommand = '"/usr/local/bin/token-audit" hook statusline'

/** Handwritten recorder plan for browser preview. Writes stay in memory. */
export function previewRecorderPlan(parts: RecorderParts, action: 'install' | 'uninstall'): RecorderPlan {
  const changes: RecorderPlan['changes'] = []
  if (parts.statusline) {
    changes.push(action === 'install'
      ? { file: previewFile, key_path: 'statusLine', action: '替换', before_command: 'printf old', after_command: statuslineCommand }
      : { file: previewFile, key_path: 'statusLine', action: '恢复', before_command: statuslineCommand, after_command: 'printf old' })
  }
  if (parts.hooks) {
    for (const event of ['SessionStart', 'UserPromptSubmit', 'Stop', 'SubagentStop']) {
      changes.push({
        file: previewFile,
        key_path: `hooks.${event}[0]`,
        action: action === 'install' ? '新增' : '删除',
        before_command: action === 'install' ? null : hookCommand,
        after_command: action === 'install' ? hookCommand : null,
      })
    }
  }
  return {
    changes,
    backup_path: changes.length ? `${mockEnvironment.backup_dir}/settings-preview.json` : null,
    notes: [],
  }
}
