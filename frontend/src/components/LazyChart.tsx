import { Component, lazy, Suspense } from 'react'
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

// A chunk that fails to load (offline cold start, stale hash after a deploy)
// rejects the lazy import; without a boundary that rejection blanks the whole
// console tree. Swallow it and keep the chart slot empty instead.
class ChartBoundary extends Component<{ children: React.ReactNode }, { failed: boolean }> {
  state = { failed: false }

  static getDerivedStateFromError() {
    return { failed: true }
  }

  componentDidCatch() {}

  render() {
    return this.state.failed ? <div className="chart-loading" role="status" /> : this.props.children
  }
}

export function LazyTrafficChart(props: React.ComponentProps<typeof TrafficChart>) {
  return <ChartBoundary><Suspense fallback={<ChartFallback />}><Traffic {...props} /></Suspense></ChartBoundary>
}

export function LazyProviderPie(props: React.ComponentProps<typeof ProviderPie>) {
  return <ChartBoundary><Suspense fallback={<ChartFallback />}><Pie {...props} /></Suspense></ChartBoundary>
}

export function LazyCategoryBars(props: React.ComponentProps<typeof CategoryBars>) {
  return <ChartBoundary><Suspense fallback={<ChartFallback />}><Bars {...props} /></Suspense></ChartBoundary>
}
