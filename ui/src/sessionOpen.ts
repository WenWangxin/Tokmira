import { m } from './i18n/index.ts'
import { openSession, type SessionRef } from './api'
import { outcomeMessage } from './sessionOpenLogic.ts'
import { showToast } from './toast'

export async function openAndReport(session: SessionRef, command?: string) {
  try {
    const { text, tone } = outcomeMessage(await openSession(session, command), command)
    showToast(text, tone, 6000)
  } catch (cause) {
    showToast((cause instanceof Error ? cause.message : String(cause)) || m().sessions.openFailed, 'error')
  }
}
