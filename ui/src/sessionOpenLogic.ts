import { m } from './i18n/index.ts'
import type { OpenOutcome } from './api.ts'
import type { ToastTone } from './toast.ts'

/** What to tell the user after "open session" or "fill command". */
export function outcomeMessage(outcome: OpenOutcome, command?: string): { text: string; tone: ToastTone } {
  const t = m().sessions.openOutcome
  switch (outcome) {
    case 'filled':
      return { text: t.filled(command ?? ''), tone: 'success' }
    case 'focused':
    case 'focused_copied':
      return { text: command ? t.focusedCopied(command) : t.focused, tone: 'success' }
    case 'app_copied':
    case 'app_opened':
      return { text: command ? t.appCopied(command) : t.appOpened, tone: 'info' }
    case 'resumed':
      return { text: command ? t.resumedCopied(command) : t.resumed, tone: 'success' }
    case 'not_found':
      return { text: command ? t.notFoundCopied(command) : t.notFound, tone: 'warning' }
  }
}
