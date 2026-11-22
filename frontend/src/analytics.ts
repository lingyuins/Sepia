import type { Provider } from './workspace'

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
