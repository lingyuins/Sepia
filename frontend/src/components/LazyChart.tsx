import { lazy, Suspense } from 'react'
import { useTranslation } from '../workspace'
import type TrafficChart from './TrafficChart'
import type { CategoryBars, ProviderPie } from './CategoryChart'

const Traffic = lazy(() => import('./TrafficChart'))
const Pie = lazy(() => import('./CategoryChart').then((module) => ({ default: module.ProviderPie })))
const Bars = lazy(() => import('./CategoryChart').then((module) => ({ default: module.CategoryBars })))

function ChartFallback() {
  const t = useTranslation()
  return <div className="chart-loading" role="status">{t('loading')}</div>
}

export function LazyTrafficChart(props: React.ComponentProps<typeof TrafficChart>) {
  return <Suspense fallback={<ChartFallback />}><Traffic {...props} /></Suspense>
}

export function LazyProviderPie(props: React.ComponentProps<typeof ProviderPie>) {
  return <Suspense fallback={<ChartFallback />}><Pie {...props} /></Suspense>
}

export function LazyCategoryBars(props: React.ComponentProps<typeof CategoryBars>) {
  return <Suspense fallback={<ChartFallback />}><Bars {...props} /></Suspense>
}
