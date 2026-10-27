import { computed, ref } from 'vue'
import zh, { type Messages } from './zh.ts'
import en from './en.ts'

export type Locale = 'zh' | 'en'
export type LanguageChoice = 'system' | 'zh' | 'en'

const dictionaries: Record<Locale, Messages> = { zh, en }

/** Chinese until settings load, so plain-logic tests keep the original copy. */
export const locale = ref<Locale>('zh')

/** The active dictionary for templates: `const t = messages`, then `{{ t.settings.title }}`. */
export const messages = computed(() => dictionaries[locale.value])

/** The active dictionary for plain `.ts` logic; read it at call time, not at import time. */
export function m(): Messages {
  return dictionaries[locale.value]
}

/** `zh-*` follows Chinese; every other system language gets English. */
export function resolveLocale(choice: LanguageChoice, systemLanguage: string | undefined): Locale {
  if (choice === 'zh' || choice === 'en') return choice
  return systemLanguage?.toLowerCase().startsWith('zh') ? 'zh' : 'en'
}

export function applyLanguage(choice: LanguageChoice, root: HTMLElement = document.documentElement) {
  locale.value = resolveLocale(choice, navigator.language)
  root.lang = intlLocale()
}

/** BCP 47 tag for `Intl` and `toLocale*` calls. */
export function intlLocale(): string {
  return locale.value === 'zh' ? 'zh-CN' : 'en-US'
}

export function formatInteger(value: number): string {
  return Math.round(value).toLocaleString(intlLocale())
}

/**
 * Token counts in the active language's units: 万 / 亿 in Chinese, K / M / B in English.
 * `wholeFrom` drops the decimal for large values so narrow cells stay short.
 */
export function compactNumber(value: number, wholeFrom = 10_000_000): { value: string; unit: string } {
  const trim = (n: number, digits: number) => Number(n.toFixed(digits)).toString()
  if (locale.value === 'zh') {
    if (value >= 100_000_000) return { value: trim(value / 100_000_000, 1), unit: '亿' }
    if (value >= 10_000) return { value: trim(value / 10_000, value >= wholeFrom ? 0 : 1), unit: '万' }
    return { value: formatInteger(value), unit: '' }
  }
  if (value >= 1_000_000_000) return { value: trim(value / 1_000_000_000, 1), unit: 'B' }
  if (value >= 1_000_000) return { value: trim(value / 1_000_000, value >= wholeFrom * 10 ? 0 : 1), unit: 'M' }
  if (value >= 10_000) return { value: trim(value / 1_000, value >= 100_000 ? 0 : 1), unit: 'K' }
  return { value: formatInteger(value), unit: '' }
}

/** `12.3 万` / `123K`: Chinese units take a space, English units sit on the number. */
export function compactText(value: number, wholeFrom?: number): string {
  const parts = compactNumber(value, wholeFrom)
  if (!parts.unit) return parts.value
  return locale.value === 'zh' ? `${parts.value} ${parts.unit}` : `${parts.value}${parts.unit}`
}

/** 24-hour `HH:MM`. */
export function clock(value: string | Date, seconds = false): string {
  return new Date(value).toLocaleTimeString(intlLocale(), { hour: '2-digit', minute: '2-digit', ...(seconds ? { second: '2-digit' } : {}), hour12: false })
}

/** `10/27` in both languages. */
export function monthDay(value: string | Date): string {
  const date = new Date(value)
  return `${date.getMonth() + 1}/${date.getDate()}`
}

/** `10 月 27 日` / `Oct 27`. */
export function longMonthDay(value: string | Date): string {
  const date = new Date(value)
  return locale.value === 'zh'
    ? `${date.getMonth() + 1} 月 ${date.getDate()} 日`
    : date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}

/** Joins names the way each language lists them: `Claude、Codex` / `Claude, Codex`. */
export function listJoin(items: readonly string[]): string {
  return items.join(locale.value === 'zh' ? '、' : ', ')
}
