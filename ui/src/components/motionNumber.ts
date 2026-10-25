export function easeOutCubic(progress: number): number {
  const value = Math.max(0, Math.min(1, progress))
  return 1 - (1 - value) ** 3
}

export function formatAnimatedNumber(from: number, target: string, progress: number): string {
  const match = /^([^\d-]*)(-?[\d,]+(?:\.(\d+))?)(.*)$/.exec(target)
  if (!match) return target
  const end = Number(match[2]?.replaceAll(',', ''))
  if (!Number.isFinite(end)) return target
  const digits = match[3]?.length ?? 0
  const current = from + (end - from) * easeOutCubic(progress)
  const number = match[2]?.includes(',')
    ? current.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits })
    : current.toFixed(digits)
  return `${match[1]}${number}${match[4]}`
}

export function numericValue(text: string): number | null {
  const match = /^([^\d-]*)(-?[\d,]+(?:\.\d+)?)(.*)$/.exec(text)
  if (!match) return null
  const value = Number(match[2]?.replaceAll(',', ''))
  return Number.isFinite(value) ? value : null
}
