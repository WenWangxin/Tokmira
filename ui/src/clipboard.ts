import { m } from './i18n/index.ts'
import { showToast } from './toast'

/** Copies a next-step command with the WebView clipboard; no clipboard plugin or extra capability. */
export async function copyCommand(command: string) {
  try {
    if (!navigator.clipboard) throw new Error('clipboard unavailable')
    await navigator.clipboard.writeText(command)
    showToast(m().advice.copied(command), 'success')
  } catch {
    showToast(m().advice.copyFailed, 'warning')
  }
}
