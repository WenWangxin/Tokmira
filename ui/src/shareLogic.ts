export type ShareToolName = 'Claude' | 'Codex'

export interface ShareToolCost {
  tool: ShareToolName
  plan: string | null
  subscription_usd: number | null
  estimated_cost_usd: number
}

export interface ShareProject {
  name: string
  estimated_cost_usd: number
}

export interface ShareSummary {
  year: number
  month: number
  month_estimated_cost_usd: number
  tools: ShareToolCost[]
  recognized_subscriptions: number
  subscription_total_usd: number | null
  subscription_value_multiple: number | null
  unrecognized_tools: ShareToolName[]
  cache_hit_rate: number | null
  requests: number
  top_project: ShareProject | null
}

export interface ShareOptions {
  toolDetails: boolean
  cacheHitRate: boolean
  topProject: boolean
}

export interface CostBarSegment {
  tool: ShareToolName
  width: number
}

export const SHARE_FOOTER = '基于本机日志核算 · 数据未离开设备'
export const SHARE_DISCLAIMER = '金额为 API 等价估算，并非实际账单；基于本机日志核算，数据未离开设备。'

const CLAUDE_PLANS: Record<string, string> = {
  pro: 'Claude Pro',
  max: 'Claude Max 5x',
  max_5x: 'Claude Max 5x',
  max_20x: 'Claude Max 20x',
}

const CODEX_PLANS: Record<string, string> = {
  plus: 'ChatGPT Plus · Codex',
  pro: 'ChatGPT Pro · Codex',
}

export function defaultShareOptions(): ShareOptions {
  return { toolDetails: true, cacheHitRate: true, topProject: false }
}

export function shareTitle(summary: Pick<ShareSummary, 'month'>): string {
  return `token-audit · ${summary.month} 月用量报告`
}

export function formatUsd(value: number): string {
  const negative = value < 0
  const [whole, fraction] = Math.abs(value).toFixed(2).split('.')
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  return `${negative ? '-' : ''}$${grouped}.${fraction}`
}

/** Whole dollar amounts stay compact ($40). Anything else keeps cents. */
export function formatSubscriptionUsd(value: number): string {
  const rounded = Math.round(value * 100) / 100
  if (Math.abs(rounded - Math.round(rounded)) < 0.001) {
    const whole = Math.round(rounded)
    const negative = whole < 0
    const grouped = Math.abs(whole).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
    return `${negative ? '-' : ''}$${grouped}`
  }
  return formatUsd(rounded)
}

export function formatAmountDigits(value: number): string {
  return formatUsd(value).replace(/^-?\$/, '')
}

export function formatCount(value: number): string {
  return Math.round(value).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',')
}

export function formatPercent(rate: number | null): string {
  if (rate == null || !Number.isFinite(rate)) return '—'
  return `${Math.round(rate * 100)}%`
}

export function subscriptionCaption(summary: Pick<ShareSummary, 'recognized_subscriptions' | 'subscription_total_usd'>): string {
  const total = summary.subscription_total_usd
  if (summary.recognized_subscriptions >= 2 && total != null) {
    return `两份订阅共 ${formatSubscriptionUsd(total)}，折合 API 费用`
  }
  if (summary.recognized_subscriptions === 1 && total != null) {
    return `订阅 ${formatSubscriptionUsd(total)}，折合 API 费用`
  }
  return '本月折合 API 费用'
}

export function valuePillText(multiple: number | null): string | null {
  if (multiple == null || !Number.isFinite(multiple)) return null
  return `价值约为订阅的 ${multiple.toFixed(1)} 倍`
}

/** Shown beside the value pill when some, but not all, plans are unrecognized. */
export function unrecognizedNote(summary: Pick<ShareSummary, 'subscription_value_multiple' | 'unrecognized_tools'>): string | null {
  if (summary.subscription_value_multiple == null || !summary.unrecognized_tools.length) return null
  return `不含 ${summary.unrecognized_tools.join('、')}（套餐未识别）`
}

export function toolRowLabel(tool: ShareToolCost): string {
  if (tool.subscription_usd == null || !tool.plan?.trim()) return tool.tool
  const key = tool.plan.trim().toLowerCase().replace(/[\s-]+/g, '_')
  const known = tool.tool === 'Claude' ? CLAUDE_PLANS : CODEX_PLANS
  return known[key] ?? tool.tool
}

export function projectLine(project: ShareProject | null, maxName?: number): string {
  if (!project) return '消耗最高的项目：暂无'
  const chars = Array.from(project.name)
  const name = maxName && chars.length > maxName
    ? `${chars.slice(0, Math.max(1, maxName - 1)).join('')}…`
    : project.name
  return `消耗最高的项目：${name} · ${formatUsd(project.estimated_cost_usd)}`
}

export function exportFileName(summary: Pick<ShareSummary, 'year' | 'month'>, extension: 'png' | 'md'): string {
  return `token-audit-${summary.year}-${String(summary.month).padStart(2, '0')}.${extension}`
}

/** Widths sum to `width`. Tools with no positive cost are omitted. */
export function costBarSegments(tools: ShareToolCost[], width: number): CostBarSegment[] {
  const positive = tools.filter(tool => tool.estimated_cost_usd > 0)
  const total = positive.reduce((sum, tool) => sum + tool.estimated_cost_usd, 0)
  if (total <= 0 || width <= 0 || !positive.length) return []
  const exact = positive.map(tool => (tool.estimated_cost_usd / total) * width)
  const widths = exact.map(value => Math.floor(value))
  let leftover = width - widths.reduce((sum, value) => sum + value, 0)
  const order = exact
    .map((value, index) => ({ index, fraction: value - Math.floor(value) }))
    .sort((a, b) => b.fraction - a.fraction || a.index - b.index)
  for (let i = 0; leftover > 0; i += 1) {
    widths[order[i % order.length].index] += 1
    leftover -= 1
  }
  return positive.map((tool, index) => ({ tool: tool.tool, width: widths[index] }))
}

export function shareMarkdown(summary: ShareSummary, options: ShareOptions): string {
  const lines = [
    `# ${shareTitle(summary)}`,
    '',
    `${subscriptionCaption(summary)}：${formatUsd(summary.month_estimated_cost_usd)}`,
  ]
  if (options.toolDetails && summary.tools.length) {
    lines.push('', '## 各工具明细', '')
    for (const tool of summary.tools) {
      lines.push(`- ${toolRowLabel(tool)}：${formatUsd(tool.estimated_cost_usd)}`)
    }
  }
  const pill = valuePillText(summary.subscription_value_multiple)
  if (pill) {
    const note = unrecognizedNote(summary)
    lines.push('', note ? `${pill}。${note}。` : `${pill}。`)
  }
  if (options.cacheHitRate) {
    lines.push('', `缓存命中率：${formatPercent(summary.cache_hit_rate)}`)
  }
  lines.push('', `本月请求：${formatCount(summary.requests)} 次`)
  if (options.topProject) lines.push('', projectLine(summary.top_project))
  lines.push('', SHARE_DISCLAIMER)
  return lines.join('\n')
}

export function svgDataUri(markup: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(markup)}`
}

export function exportErrorText(cause: unknown): string {
  if (typeof cause === 'string' && cause.trim()) return cause.trim()
  if (cause instanceof DOMException && cause.name === 'SecurityError') return '导出图片失败'
  if (cause instanceof Error && cause.message.trim()) return cause.message.trim()
  return '导出失败'
}

/** Draws the same SVG used for preview at 2× resolution. */
export async function renderSharePng(svg: SVGSVGElement): Promise<Blob> {
  const clone = svg.cloneNode(true) as SVGSVGElement
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  clone.setAttribute('xmlns:xlink', 'http://www.w3.org/1999/xlink')
  const xml = new XMLSerializer().serializeToString(clone)
  const blob = new Blob([xml], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  try {
    const image = new Image()
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve()
      image.onerror = () => reject(new Error('无法绘制分享卡片'))
      image.src = url
    })
    const width = svg.viewBox.baseVal.width || Number(svg.getAttribute('width'))
    const height = svg.viewBox.baseVal.height || Number(svg.getAttribute('height'))
    if (!width || !height) throw new Error('无法绘制分享卡片')
    const canvas = document.createElement('canvas')
    canvas.width = Math.round(width * 2)
    canvas.height = Math.round(height * 2)
    const context = canvas.getContext('2d')
    if (!context) throw new Error('无法绘制分享卡片')
    context.drawImage(image, 0, 0, canvas.width, canvas.height)
    const png = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, 'image/png'))
    if (!png) throw new Error('无法生成图片')
    return png
  } finally {
    URL.revokeObjectURL(url)
  }
}
