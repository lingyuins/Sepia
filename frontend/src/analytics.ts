import type { TranslationKey } from './i18n'
import type { Provider } from './workspace'

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

export function analyticsPeriodWindowMs(period: string): number {
  if (period === '24 hours') return 86_400_000
  if (period === '7 days') return 604_800_000
  if (period === '90 days') return 7_776_000_000
  if (period === 'all') return Number.POSITIVE_INFINITY
  return 2_592_000_000
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

export function routeProviderMetrics(route: Pick<Provider, 'latency' | 'success'>[]) {
  const latencies = route.map((provider) => provider.latency).filter((latency) => latency > 0)
  const successRates = route
    .map((provider) => Number.parseFloat(provider.success))
    .filter((rate) => Number.isFinite(rate))
  return {
    latency: latencies.length
      ? Math.round(latencies.reduce((sum, latency) => sum + latency, 0) / latencies.length)
      : 0,
    success: successRates.length
      ? `${(successRates.reduce((sum, rate) => sum + rate, 0) / successRates.length).toFixed(2)}%`
      : '—',
  }
}

export function rankRoutePerformance<T extends { requests: number }>(rows: T[], limit = 5) {
  const ranked = rows.filter((row) => row.requests > 0).sort((a, b) => b.requests - a.requests)
  return { ranked, top: ranked.slice(0, limit), hasMore: ranked.length > limit }
}
