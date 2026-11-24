import type { TranslationKey } from './i18n'

export const analyticsPeriods = ['24 hours', '7 days', '30 days', '90 days', 'all'] as const
export type AnalyticsPeriod = (typeof analyticsPeriods)[number]
const periodLabelKeys: Record<AnalyticsPeriod, TranslationKey> = {
  '24 hours': '24h',
  '7 days': '7d',
  '30 days': '30d',
  '90 days': '90d',
  all: 'allTime',
}

export function analyticsPeriodLabelKey(period: string): TranslationKey {
  return periodLabelKeys[period as AnalyticsPeriod] ?? '30d'
}

export function formatRequestTime(value: string, locale: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleTimeString(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  })
}

export function chartTimeLabel(value: string, period: string, language: string, timeZone?: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat(language, period === '24 hours'
    ? { hour: '2-digit', minute: '2-digit', hour12: false, timeZone }
    : { month: 'numeric', day: 'numeric', timeZone },
  ).format(date)
}

export type RouteAggregate = {
  route: string
  requests: number
  successful: number
  latency_ms: number
}

// Route health must come from the server-side window aggregate (analytics.by_route):
// provider-level averages and the newest request page both undercount routes whose
// traffic aged out of that page.
export function routeAggregate(rows: RouteAggregate[] | undefined, slug: string) {
  const row = rows?.find((item) => item.route === slug)
  if (!row || row.requests <= 0) return { requests: 0, latency: 0, success: '—' }
  return {
    requests: row.requests,
    latency: Math.round(row.latency_ms),
    success: `${((row.successful / row.requests) * 100).toFixed(2)}%`,
  }
}

export function modelPerformanceRows(
  rows: { model: string; requests: number }[] | undefined,
  limit = 6,
) {
  return [...(rows ?? [])]
    .sort((a, b) => b.requests - a.requests)
    .slice(0, limit)
    .map((row) => ({
      model: row.model.replace('claude-', '').replace('gemini-', ''),
      requests: row.requests,
    }))
}

export function rankRoutePerformance<T extends { requests: number }>(rows: T[], limit = 5) {
  const ranked = rows.filter((row) => row.requests > 0).sort((a, b) => b.requests - a.requests)
  return { ranked, top: ranked.slice(0, limit), hasMore: ranked.length > limit }
}
