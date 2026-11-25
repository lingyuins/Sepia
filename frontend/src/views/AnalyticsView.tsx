import { useEffect, useState } from 'react'
import { ArrowDownRight, Coins, Gauge, Layers3, Route } from 'lucide-react'
import { useWorkspace, useTranslation, useLocale } from '../workspace'
import { chartTimeLabel, analyticsPeriodLabelKey, analyticsPeriods, modelPerformanceRows, rankRoutePerformance, routeAggregate } from '../analytics'
import { routeDisplayCandidates } from '../route-display'
import { ProviderCell, SectionHeading } from '../components/UI'
import { strategyLabelKey } from '../route-strategy'
import { LazyCategoryBars, LazyProviderPie, LazyTrafficChart } from '../components/LazyChart'

const requestCount = (value: string) => {
  const normalized = value.toLowerCase().replace(/,/g, '')
  const amount = Number.parseFloat(normalized)
  if (!Number.isFinite(amount)) return 0
  return amount * (normalized.includes('m') ? 1_000_000 : normalized.includes('k') ? 1_000 : 1)
}

export function AnalyticsView() {
  const { providers, routes, channelGroups, analytics, refresh, notify } =
    useWorkspace()
  const t = useTranslation()
  const locale = useLocale()
  const [period, setPeriod] = useState('30 days')
  const [metric, setMetric] = useState('Requests')
  const [showAllRoutes, setShowAllRoutes] = useState(false)
  const periodLabel = t(analyticsPeriodLabelKey(period))

  // Aggregate cards read only the analytics domain; never refetch the shared
  // request list with a narrower period — that would silently shrink the
  // Requests page's period-independent history.
  useEffect(() => {
    refresh('30 days', ['analytics']).catch((cause) =>
      notify(cause instanceof Error ? cause.message : String(cause)),
    )
  }, [])

  const data = (analytics.series || []).map((point) => ({
    ...point,
    time: chartTimeLabel(point.time, period, locale),
    cost: point.cost_micros / 1_000_000,
    latency: Math.round(point.latency_ms),
  }))

  const colors = [
    'var(--chart-accent)',
    'var(--chart-series-2)',
    'var(--chart-series-3)',
    'var(--chart-series-4)',
  ]

  const totalProviderRequests = providers.reduce(
    (sum, provider) => sum + requestCount(provider.requests),
    0,
  )
  const providerData = providers.map((provider, index) => ({
    name: provider.name,
    value: totalProviderRequests
      ? (requestCount(provider.requests) / totalProviderRequests) * 100
      : 0,
    color: colors[index % colors.length],
    requests: provider.requests,
  }))

  const groupLabel = (id: string) =>
    id === 'official'
      ? t('officialChannels')
      : id === 'free'
        ? t('freeChannels')
        : id === 'team'
          ? t('teamChannels')
          : channelGroups.find((group) => group.id === id)?.name || t('noGroup')

  const groupTotals = channelGroups.map((group, index) => {
    const members = providers.filter((provider) => provider.group === group.id)
    const dailyRequests = members.reduce(
      (sum, provider) => sum + requestCount(provider.requests),
      0,
    )
    const latencies = members.map((provider) => provider.latency).filter((latency) => latency > 0)
    const successRates = members
      .map((provider) => Number.parseFloat(provider.success))
      .filter((rate) => Number.isFinite(rate))
    return {
      id: group.id,
      name: groupLabel(group.id),
      color: colors[index % colors.length],
      requests: Math.round(dailyRequests),
      providerCount: members.length,
      models: members.reduce((sum, provider) => sum + provider.models, 0),
      latency: latencies.length
        ? Math.round(latencies.reduce((sum, latency) => sum + latency, 0) / latencies.length)
        : 0,
      success: successRates.length
        ? `${(successRates.reduce((sum, rate) => sum + rate, 0) / successRates.length).toFixed(2)}%`
        : '—',
    }
  })

  const totalGroupRequests = groupTotals.reduce((sum, group) => sum + group.requests, 0)
  const groupData = groupTotals
    .map((group) => ({
      ...group,
      share: totalGroupRequests ? (group.requests / totalGroupRequests) * 100 : 0,
    }))
    .sort((a, b) => b.requests - a.requests)

  const strategyLabel = (value: string) => t(strategyLabelKey(value))
  const routeTotals = routes.map((route, index) => {
    const candidates = routeDisplayCandidates(route)
    const metrics = routeAggregate(analytics.by_route, route.slug ?? '')
    const name = route.name || route.slug || `Route ${route.id}`
    return {
      id: route.id,
      name,
      chartName: name.length > 20 ? `${name.slice(0, 18)}…` : name,
      color: colors[index % colors.length],
      ...metrics,
      candidateCount: candidates.length,
      strategy: strategyLabel(route.strategy),
    }
  })

  const totalRouteRequests = routeTotals.reduce((sum, route) => sum + route.requests, 0)
  const routePerformance = rankRoutePerformance(
    routeTotals.map((route) => ({
      ...route,
      share: totalRouteRequests ? (route.requests / totalRouteRequests) * 100 : 0,
    })),
  )
  const routeData = routePerformance.ranked
  const visibleRouteData = showAllRoutes ? routeData : routePerformance.top

  const modelData = modelPerformanceRows(analytics.by_model)

  return (
    <>
      <SectionHeading
        eyebrow={t('analyticsEyebrow')}
        title={t('analyticsTitle')}
        description={t('analyticsSubtitle')}
        action={
          <div className="segmented">
            {analyticsPeriods.map((item) => (
              <button
                type="button"
                key={item}
                className={period === item ? 'active' : ''}
                onClick={() => {
                  setPeriod(item)
                  refresh(item, ['analytics']).catch((cause) =>
                    notify(cause instanceof Error ? cause.message : String(cause)),
                  )
                }}
              >
                {t(analyticsPeriodLabelKey(item))}
              </button>
            ))}
          </div>
        }
      />

      <div className="analytics-highlight glass-panel">
        <div>
          <span className="eyebrow">
            {t('totalRequests')} · {periodLabel}
          </span>
          <h2 className="tabular">{analytics.requests.toLocaleString(locale)}</h2>
          <span className="tabular">
            {periodLabel} · {analytics.successful.toLocaleString(locale)} {t('successful')}
          </span>
        </div>
        <div className="analytics-big-number">
          <span>{t('totalCost')}</span>
          <strong className="tabular">${(analytics.cost_micros / 1_000_000).toFixed(4)}</strong>
          <small>{t('costDescription')}</small>
        </div>
      </div>

      <div className="analytics-tabs">
        {(
          [
            ['Requests', Route],
            ['Cost', ArrowDownRight],
            ['Tokens', Coins],
            ['Latency', Gauge],
          ] as [string, typeof Route][]
        ).map(([label, Icon]) => (
          <button
            type="button"
            key={label}
            className={metric === label ? 'active' : ''}
            onClick={() => setMetric(label)}
          >
            <Icon size={15} />
            {label === 'Requests' ? t('requests') : label === 'Cost' ? t('cost') : label === 'Tokens' ? t('tokens') : t('latency')}
          </button>
        ))}
      </div>

      <div className="dashboard-grid">
        <section className="chart-panel glass-panel">
          <div className="panel-heading">
            <div>
              <h3>
                {metric === 'Requests'
                  ? t('usageOverTime')
                  : metric === 'Cost'
                    ? t('lessSpend')
                    : metric === 'Tokens'
                      ? t('tokenVolume')
                      : t('fastGuesswork')}
              </h3>
              <p>
                {periodLabel} {t('acrossProviders')}
              </p>
            </div>
            <span className="chart-unit">
              {metric === 'Cost' ? 'USD' : metric === 'Latency' ? t('milliseconds') : metric === 'Tokens' ? t('tokens') : t('requests')}
            </span>
          </div>
          <div className="traffic-chart tall">
            <LazyTrafficChart
              data={data}
              metric={metric === 'Requests' ? 'requests' : metric === 'Cost' ? 'cost' : metric === 'Tokens' ? 'tokens' : 'latency'}
              gradientId="analyticsGradient"
              margin={{ top: 20, right: 10, left: -20 }}
            />
          </div>
        </section>

        <section className="health-panel glass-panel">
          <div className="panel-heading">
            <div>
              <h3>{t('requestsByProvider')}</h3>
              <p>{t('trafficGoes')}</p>
            </div>
          </div>
          <div className="provider-pie">
            <LazyProviderPie data={providerData} />
            <div className="provider-pie-legend tabular">
              {providerData.map((provider) => (
                <div key={provider.name}>
                  <i style={{ background: provider.color }} />
                  <span>{provider.name}</span>
                  <strong>{provider.value.toFixed(1)}%</strong>
                </div>
              ))}
            </div>
          </div>
        </section>
      </div>

      <div className="performance-pair">
        <section className="group-performance glass-panel">
          <div className="panel-heading">
            <div>
              <h3>{t('groupPerformance')}</h3>
              <p>{t('groupPerformanceSubtitle')}</p>
            </div>
            <span className="chart-unit">{periodLabel}</span>
          </div>
          <div className="group-performance-layout">
            <div
              className="group-performance-chart"
              style={{ height: Math.max(190, groupData.length * 52) }}
            >
              <LazyCategoryBars data={groupData} />
            </div>
            <div className="group-performance-list tabular">
              {groupData.map((group, index) => (
                <div className="group-performance-row" key={group.id}>
                  <div className="group-performance-name">
                    <span className="group-rank">{index + 1}</span>
                    <div>
                      <strong>
                        {group.name}
                        {index === 0 && group.requests > 0 && (
                          <em className="rank-leader"> · {t('mostUsed')}</em>
                        )}
                      </strong>
                      <small>
                        {group.providerCount} {t('providersShort')} · {group.models} {t('models')}
                      </small>
                    </div>
                  </div>
                  <div className="group-performance-usage">
                    <strong>{group.requests.toLocaleString(locale)}</strong>
                    <small>{group.share.toFixed(1)}%</small>
                  </div>
                  <div className="group-performance-health">
                    <span>
                      {group.success} {t('successShort')}
                    </span>
                    <span>{group.latency ? `${group.latency} ${t('avgShort')}` : '—'}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="group-performance route-performance glass-panel">
          <div className="panel-heading">
            <div>
              <h3>{t('routePerformance')}</h3>
              <p>
                {t('routePerformanceSubtitle')} · {t('aggregatedAnalyticsNote')}
              </p>
            </div>
            <span className="chart-unit">{periodLabel}</span>
          </div>
          <div className="group-performance-layout">
            {routeData.length ? (
              <>
                <div
                  className="group-performance-chart"
                  style={{ height: Math.max(190, routePerformance.top.length * 52) }}
                >
                  <LazyCategoryBars
                    data={routePerformance.top}
                    labelKey="chartName"
                    labelWidth={125}
                  />
                </div>
                <div className="group-performance-list tabular">
                  {visibleRouteData.map((route, index) => (
                    <div className="group-performance-row" key={route.id}>
                      <div className="group-performance-name">
                        <span className="group-rank">{index + 1}</span>
                        <div>
                          <strong>
                            {route.name}
                            {index === 0 && route.requests > 0 && (
                              <em className="rank-leader"> · {t('mostUsed')}</em>
                            )}
                          </strong>
                          <small>
                            {route.candidateCount} {t('candidatesShort')} · {route.strategy}
                          </small>
                        </div>
                      </div>
                      <div className="group-performance-usage">
                        <strong>{route.requests.toLocaleString(locale)}</strong>
                        <small>{route.share.toFixed(1)}%</small>
                      </div>
                      <div className="group-performance-health">
                        <span>
                          {route.success} {t('successShort')}
                        </span>
                        <span>{route.latency ? `${route.latency} ${t('avgShort')}` : '—'}</span>
                        <span>
                          {route.tokens.toLocaleString(locale)} {t('tokensShort')}
                        </span>
                      </div>
                    </div>
                  ))}
                  {routePerformance.hasMore && (
                    <button
                      type="button"
                      className="button text-button route-performance-toggle"
                      aria-expanded={showAllRoutes}
                      onClick={() => setShowAllRoutes(!showAllRoutes)}
                    >
                      {showAllRoutes ? t('collapseRoutes') : t('showAllRoutes')}
                    </button>
                  )}
                </div>
              </>
            ) : (
              <div className="empty-state compact-empty route-performance-empty">
                <strong>{t('noRouteTraffic')}</strong>
                <p>{t('noRouteTrafficHint')}</p>
              </div>
            )}
          </div>
        </section>
      </div>

      <section className="model-performance glass-panel">
        <div className="panel-heading">
          <div>
            <h3>{t('modelPerformance')}</h3>
            <p>
              {t('modelPerformanceSubtitle')} · {t('aggregatedAnalyticsNote')}
            </p>
          </div>
          <Layers3 size={17} />
        </div>
        <div className="model-bars">
          <LazyCategoryBars
            data={modelData}
            labelKey="model"
            labelWidth={100}
            barSize={10}
            margin={{ left: 15, right: 20 }}
          />
        </div>
      </section>

      <div className="provider-performance">
        <div className="panel-heading">
          <div>
            <h3>{t('providerComparison')}</h3>
            <p>{t('providerComparisonSubtitle')}</p>
          </div>
        </div>
        <div className="provider-performance-grid tabular">
          {providers.map((provider) => (
            <div key={provider.id} className="glass-panel">
              <ProviderCell name={provider.name} />
              <span>
                {provider.success} {t('successShort')}
              </span>
              <strong>
                {provider.latency || '—'}
                <small>{provider.latency ? t('avgShort') : ''}</small>
              </strong>
            </div>
          ))}
        </div>
      </div>
    </>
  )
}
