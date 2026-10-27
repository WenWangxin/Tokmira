/** "上次检查：今天 17:03"; dates in other days keep the month and day. */
export function lastCheckedLine(iso: string | null, now: Date = new Date()): string {
  if (!iso) return m().update.neverChecked
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return m().update.neverChecked
  const time = clock(at)
  const sameDay = at.getFullYear() === now.getFullYear() && at.getMonth() === now.getMonth() && at.getDate() === now.getDate()
  return sameDay ? m().update.checkedToday(time) : m().update.checkedOn(longMonthDay(at), time)
}

const megabytes = (bytes: number) => (bytes / 1024 / 1024).toFixed(1)

/** "已下载 3.2 / 12.0 MB（27%）"; without a total only the downloaded size is known. */
export function progressLine(downloaded: number, total: number | null): string {
  if (total && total > 0) {
    const percent = Math.min(100, Math.floor((downloaded / total) * 100))
    return m().update.downloadedOf(megabytes(downloaded), megabytes(total), percent)
  }
  return m().update.downloaded(megabytes(downloaded))
}

export type UpdateStage = 'ready' | 'installing' | 'installed'

export function updateActionLabel(stage: UpdateStage): string {
  if (stage === 'installing') return m().update.installing
  if (stage === 'installed') return m().update.restart
  return m().update.downloadInstall
}
import { clock, longMonthDay, m } from './i18n/index.ts'
