export function tagCloudWeight(count: number, minCount: number, maxCount: number): number {
  if (maxCount <= minCount) return 0.5
  const weight =
    (Math.log1p(count) - Math.log1p(minCount)) / (Math.log1p(maxCount) - Math.log1p(minCount))
  if (Number.isNaN(weight)) return 0
  return Math.max(0, Math.min(1, weight))
}

export function tagCloudFontSize(weight: number, minRem = 0.8, maxRem = 1.6): number {
  const clamped = Math.max(0, Math.min(1, weight))
  return Number((minRem + clamped * (maxRem - minRem)).toFixed(10))
}
