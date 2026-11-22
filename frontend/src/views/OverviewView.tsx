import { useEffect, useState } from 'react'
import {
  ArrowUpRight,
  BarChart3,
  Gauge,
  GitBranch,
  Inbox,
  Plus,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react'
import { useWorkspace, useTranslation, useLocale, type Provider, type RequestRecord } from '../workspace'
import { gatewayUrl } from '../gateway'
import { CopyButton } from '../components/Provider'
import {
  MetricCard,
  ProviderCell,
  SectionHeading,
  StatusDot,
  StatusText,
} from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import { RouteChain } from '../components/RouteChain'
import { chartTimeLabel } from '../analytics'
import { latestSuccessfulRouteRequest, selectOverviewRoute } from '../route-display'
import { RequestsTable } from './RequestsView'
import { LazyTrafficChart } from '../components/LazyChart'
import type { Page } from '../components/AppShell'

type Period = '24 hours' | '7 days' | '30 days'

export function OverviewView({
  providers,
  navigate,
  onAdd,
  onNotify,
  onRequest,
}: {
  providers: Provider[]
  navigate: (page: Page) => void
  onAdd: () => void
  onNotify: (message: string) => void
  onRequest: (row: RequestRecord) => void
}) {
  const { requests, routes, settings, analytics, refresh } = useWorkspace()
  const t = useTranslation()
  const locale = useLocale()
  const endpoint = gatewayUrl(settings)
  const [period, setPeriod] = useState<Period>('30 days')
  const [selectedRouteId, setSelectedRouteId] = useState<number>()
  const [refreshing, setRefreshing] = useState(false)

  const defaultRoute = selectOverviewRoute(routes, requests)
  const activeRoutes = routes.filter((route) => route.active)
  const selectedRoute =
    routes.find((route) => route.id === selectedRouteId && route.active) || defaultRoute

  useEffect(() => {
    if (selectedRouteId && !routes.some((route) => route.id === selectedRouteId && route.active)) {
      setSelectedRouteId(undefined)
    }
  }, [routes, selectedRouteId])

  const latestRequest = selectedRoute
    ? latestSuccessfulRouteRequest(selectedRoute, requests)
    : undefined

  const data = (analytics.series || []).map((point) => ({
    ...point,
    time: chartTimeLabel(point.time, period, locale),
  }))

  const periodLabel = period === '24 hours' ? t('24h') : period === '7 days' ? t('7d') : t('30d')
  const enabledProviders = providers.filter((provider) => provider.enabled).length

  const refreshData = async () => {
    setRefreshing(true)
    try {
      await refresh(period, ['requests', 'analytics'])
      onNotify(t('metricsRefreshed'))
    } finally {
      setRefreshing(false)
    }
  }

  return (
    <>
      <SectionHeading
        title={t('overview')}
        description={t('overviewSubtitle')}
        action={
          <>
            <button type="button" className="button secondary" onClick={refreshData}>
              <RefreshCw size={15} className={refreshing ? 'spin' : ''} />
              {t('refresh')}
            </button>
            <button type="button" className="button primary" onClick={onAdd}>
              <Plus size={15} />
              {t('addProvider')}
            </button>
          </>
        }
      />

      <section className="connection-band" aria-label={t('configuredEndpoint')}>
        <div className="connection-band-status">
          <StatusDot />
          <strong>{t('gatewayOperational')}</strong>
          <span className="connection-band-meta tabular">
            {settings.workspace} · {enabledProviders} {t('providersEnabledShort')}
          </span>
        </div>
        <div className="connection-endpoint">
          <span>{t('configuredEndpoint')}</span>
          <code className="mono">{endpoint}</code>
          <CopyButton value={endpoint} onCopy={() => onNotify(t('endpointCopied'))} />
        </div>
      </section>

      <div className="metrics-grid">
        <MetricCard
          title={t('requestsInPeriod')}
          value={analytics.requests.toLocaleString(locale)}
          foot={periodLabel}
          icon={Inbox}
        />
        <MetricCard
          title={t('successRate')}
          value={
            analytics.requests
              ? `${((analytics.successful / analytics.requests) * 100).toFixed(2)}%`
              : '—'
          }
          foot={`${analytics.successful.toLocaleString(locale)} ${t('successful')}`}
          icon={ShieldCheck}
        />
        <MetricCard
          title={t('periodLatency')}
          value={analytics.latency_ms ? `${Math.round(analytics.latency_ms)} ms` : '—'}
          foot={analytics.latency_ms ? periodLabel : t('noLatencyData')}
          icon={Gauge}
        />
        <MetricCard
          title={t('periodCost')}
          value={`$${(analytics.cost_micros / 1_000_000).toFixed(4)}`}
          foot={periodLabel}
          icon={BarChart3}
        />
      </div>

      <div className="dashboard-grid">
        <section className="chart-panel glass-panel">
          <div className="panel-heading">
            <div>
              <h3>{t('requestActivity')}</h3>
              <p>{t('trafficSubtitle')}</p>
            </div>
            <div className="segmented">
              {(['24 hours', '7 days', '30 days'] as Period[]).map((item) => (
                <button
                  type="button"
                  className={period === item ? 'active' : ''}
                  onClick={() => {
                    setPeriod(item)
                    void refresh(item, ['requests', 'analytics'])
                  }}
                  key={item}
                >
                  {item === '24 hours' ? t('24h') : item === '7 days' ? t('7d') : t('30d')}
                </button>
              ))}
            </div>
          </div>
          <div className="chart-legend tabular">
            <span>
              <i className="legend-mint" />
              {t('allRequests')} <strong>{analytics.requests.toLocaleString(locale)}</strong>
            </span>
            <span>
              <i className="legend-grey" />
              {t('successful')} <strong>{analytics.successful.toLocaleString(locale)}</strong>
            </span>
          </div>
          <div className="traffic-chart">
            <LazyTrafficChart
              data={data}
              showSuccessful
              gradientId="requestGradient"
              interval={2}
              margin={{ top: 14, right: 6, left: -20, bottom: 0 }}
            />
          </div>
        </section>

        <section className="health-panel glass-panel">
          <div className="panel-heading">
            <div>
              <h3>{t('providerHealth')}</h3>
              <p>{t('providerHealthSubtitle')}</p>
            </div>
            <button
              type="button"
              className="icon-button"
              onClick={() => navigate('Providers')}
              aria-label={t('viewProviders')}
            >
              <ArrowUpRight size={17} />
            </button>
          </div>
          <div className="health-list">
            {providers.slice(0, 5).map((provider) => (
              <div className="health-row" key={provider.id}>
                <ProviderCell name={provider.name} />
                <div className="health-latency tabular">
                  <strong>{provider.latency ? `${provider.latency} ms` : '—'}</strong>
                  <StatusText tone={provider.enabled ? 'positive' : 'neutral'}>
                    {provider.enabled ? t('operational') : t('paused')}
                  </StatusText>
                </div>
              </div>
            ))}
            {!providers.length && (
              <div className="empty-state compact-empty">{t('noProvidersYet')}</div>
            )}
          </div>
          <button type="button" className="add-provider-link" onClick={onAdd}>
            <Plus size={14} />
            {t('connectAnother')}
          </button>
        </section>
      </div>

      <section className="route-chain-panel glass-panel">
        <div className="panel-heading">
          <div>
            <h2>{t('routeChain')}</h2>
            <p>{t('routePreviewHint')}</p>
          </div>
          <div className="route-chain-actions">
            {selectedRoute && (
              <Dropdown
                variant="compact"
                label={t('selectRoutePreview')}
                value={String(selectedRoute.id)}
                options={activeRoutes.map((route) => ({
                  value: String(route.id),
                  label: route.name || route.slug || `Route ${route.id}`,
                }))}
                onChange={(value) => setSelectedRouteId(Number(value))}
              />
            )}
            <button
              type="button"
              className="button text-button"
              onClick={() => navigate('Routing')}
            >
              {t('manageRoute')}
              <ArrowUpRight size={14} />
            </button>
          </div>
        </div>
        {selectedRoute ? (
          <>
            <RouteChain route={selectedRoute} latestRequest={latestRequest} compact />
            {!latestRequest && (
              <p className="route-empty-note">{t('noSuccessfulRouteRequest')}</p>
            )}
          </>
        ) : (
          <div className="route-chain-empty">
            <GitBranch size={22} />
            <strong>{t('noEnabledRoutes')}</strong>
            <p>{t('createRoutePrompt')}</p>
            <button
              type="button"
              className="button secondary"
              onClick={() => navigate('Routing')}
            >
              {t('createRoute')}
            </button>
          </div>
        )}
      </section>

      <section className="recent-panel glass-panel">
        <div className="panel-heading">
          <div>
            <h3>{t('recentRequests')}</h3>
            <p>{t('recentRequestsSubtitle')}</p>
          </div>
          <button
            type="button"
            className="button text-button"
            onClick={() => navigate('Requests')}
          >
            {t('viewAllRequests')}
            <ArrowUpRight size={14} />
          </button>
        </div>
        <RequestsTable compact rows={requests.slice(0, 5)} onSelect={onRequest} />
      </section>
    </>
  )
}
