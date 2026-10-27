export function quotaTicks(peak: number): number[] {
  const rounded = Math.min(100, Math.max(40, Math.ceil(Math.max(0, peak) / 10) * 10))
  const ceiling = Math.min(100, Math.ceil(rounded / 20) * 20)
  return Array.from({ length: 5 }, (_, index) => index * ceiling / 4)
}

type MarkerPosition = { x: number; y: number; delta: number }
type Box = { left: number; right: number; top: number; bottom: number }

export function placeMarkerLabels(markers: MarkerPosition[]): (number | null)[] {
  const labels: (number | null)[] = Array(markers.length).fill(null)
  const placed: Box[] = []
  const overlaps = (box: Box) => placed.some(other =>
    box.left < other.right && box.right > other.left && box.top < other.bottom && box.bottom > other.top)
  const boxFor = (marker: MarkerPosition, baseline: number): Box => {
    const width = (`+${marker.delta.toFixed(1)}`).length * 6 + 6
    return { left: marker.x - width / 2 - 2, right: marker.x + width / 2 + 2,
      top: baseline - 11, bottom: baseline + 3 }
  }
  const order = markers.map((_, index) => index).sort((a, b) => markers[b]!.delta - markers[a]!.delta || a - b)
  for (const index of order) {
    const marker = markers[index]!
    for (const baseline of [marker.y - 11, marker.y + 20]) {
      const box = boxFor(marker, baseline)
      if (!overlaps(box)) {
        labels[index] = baseline
        placed.push(box)
        break
      }
    }
  }
  return labels
}

/** Remaining-quota colour: green when plenty is left, orange around half, red near empty. */
export function remainingQuotaColor(remainingPercent: number): string {
  const remaining = Math.max(0, Math.min(100, remainingPercent))
  return remaining >= 50
    ? `color-mix(in oklch, var(--success) ${Math.round((remaining - 50) * 2)}%, var(--warning))`
    : `color-mix(in oklch, var(--warning) ${Math.round(remaining * 2)}%, var(--error))`
}
