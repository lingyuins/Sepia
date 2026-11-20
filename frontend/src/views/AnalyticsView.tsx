import { useState } from 'react'
import { ArrowDownRight, Gauge, Layers3, Route } from 'lucide-react'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'
import { useWorkspace, useTranslation } from '../workspace'
import { chartTimeLabel } from '../analytics'
import { ProviderCell, SectionHeading } from '../components/UI'
import { strategyLabelKey } from '../route-strategy'

const requestCount = (value: string) => {
  const normalized = value.toLowerCase().replace(/,/g, '')
  const amount = Number.parseFloat(normalized)
  if (!Number.isFinite(amount)) return 0
  return amount * (normalized.includes('m') ? 1_000_000 : normalized.includes('k') ? 1_000 : 1)
}

const tooltipStyle = {
  background: 'var(--panel-solid)',
  border: '1px solid var(--border-strong)',
  borderRadius: 10,
  color: 'var(--text)',
  fontSize: 12,
}

export function AnalyticsView() {
  const { providers, requests, routes, channelGroups, analytics, refresh, notify, settings } =
    useWorkspace()
  const t = useTranslation()
  const [period, setPeriod] = useState('30 days')
  const [metric, setMetric] = useState('Requests')
  const periodLabel = period === '24 hours' ? t('24h') : period === '7 days' ? t('7d') : t('30d')

  const periodRequests = requests.filter((request) => {
    const age = Date.now() - Date.parse(request.createdAt)
    return (
      Number.isFinite(age) &&
      age <= (period === '24 hours' ? 86400000 : period === '7 days' ? 604800000 : 2592000000)
    )
  })

  const locale =
    settings.language === '简体中文'
      ? 'zh-CN'
      : settings.language === '日本語'
        ? 'ja-JP'
        : 'en-US'

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
    const candidates = route.candidates?.length
      ? route.candidates.filter((candidate) => candidate.enabled)
      : [
          { provider: route.primary, model: route.primaryModel },
          ...(route.fallback ? [{ provider: route.fallback, model: route.fallbackModel }] : []),
        ]
    const candidateProviders = candidates.flatMap((candidate) => {
      const provider = providers.find((item) => item.name === candidate.provider)
      return provider ? [provider] : []
    })
    const latencies = candidateProviders
      .map((provider) => provider.latency)
      .filter((latency) => latency > 0)
    const successRates = candidateProviders
      .map((provider) => Number.parseFloat(provider.success))
      .filter((rate) => Number.isFinite(rate))
    const name = route.name || route.slug || `Route ${route.id}`
    return {
      id: route.id,
      name,
      chartName: name.length > 20 ? `${name.slice(0, 18)}…` : name,
      color: colors[index % colors.length],
      requests: periodRequests.filter((request) => request.route === route.slug).length,
      candidateCount: candidates.length,
      strategy: strategyLabel(route.strategy),
      latency: latencies.length
        ? Math.round(latencies.reduce((sum, latency) => sum + latency, 0) / latencies.length)
        : 0,
      success: successRates.length
        ? `${(successRates.reduce((sum, rate) => sum + rate, 0) / successRates.length).toFixed(2)}%`
        : '—',
    }
  })

  const totalRouteRequests = routeTotals.reduce((sum, route) => sum + route.requests, 0)
  const routeData = routeTotals
    .map((route) => ({
      ...route,
      share: totalRouteRequests ? (route.requests / totalRouteRequests) * 100 : 0,
    }))
    .sort((a, b) => b.requests - a.requests)

  const modelData = Array.from(new Set(periodRequests.map((r) => r.model)))
    .slice(0, 6)
    .map((model) => ({
      model: model.replace('claude-', '').replace('gemini-', ''),
      requests: periodRequests.filter((r) => r.model === model).length,
    }))

  return (
    <>
      <SectionHeading
        eyebrow={t('analyticsEyebrow')}
        title={t('analyticsTitle')}
        description={t('analyticsSubtitle')}
        action={
          <div className="segmented">
            {['24 hours', '7 days', '30 days'].map((item) => (
              <button
                type="button"
                key={item}
                className={period === item ? 'active' : ''}
                onClick={() => {
                  setPeriod(item)
                  refresh(item).catch((cause) =>
                    notify(cause instanceof Error ? cause.message : String(cause)),
                  )
                }}
              >
                {item === '24 hours' ? '24h' : item === '7 days' ? '7d' : '30d'}
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
          <h2 className="tabular">{analytics.requests.toLocaleString()}</h2>
          <span className="tabular">
            {periodLabel} · {analytics.successful.toLocaleString()} {t('successful')}
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
            {label === 'Requests' ? t('requests') : label === 'Cost' ? t('cost') : t('latency')}
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
                    : t('fastGuesswork')}
              </h3>
              <p>
                {periodLabel} {t('acrossProviders')}
              </p>
            </div>
            <span className="chart-unit">
              {metric === 'Cost' ? 'USD' : metric === 'Latency' ? t('milliseconds') : t('requests')}
            </span>
          </div>
          <div className="traffic-chart tall">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={data} margin={{ top: 20, right: 10, left: -20 }}>
                <defs>
                  <linearGradient id="analyticsGradient" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="var(--chart-accent)" stopOpacity={0.22} />
                    <stop offset="100%" stopColor="var(--chart-accent)" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid vertical={false} stroke="var(--chart-grid)" />
                <XAxis
                  dataKey="time"
                  axisLine={false}
                  tickLine={false}
                  tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
                />
                <YAxis
                  axisLine={false}
                  tickLine={false}
                  tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
                />
                <Tooltip contentStyle={tooltipStyle} />
                <Area
                  dataKey={
                    metric === 'Requests' ? 'requests' : metric === 'Cost' ? 'cost' : 'latency'
                  }
                  type="monotone"
                  stroke="var(--chart-accent)"
                  strokeWidth={2}
                  fill="url(#analyticsGradient)"
                />
              </AreaChart>
            </ResponsiveContainer>
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
            <ResponsiveContainer width="45%" height={136}>
              <PieChart>
                <Pie
                  data={providerData}
                  dataKey="value"
                  innerRadius={38}
                  outerRadius={56}
                  paddingAngle={3}
                  stroke="none"
                >
                  {providerData.map((entry) => (
                    <Cell key={entry.name} fill={entry.color} />
                  ))}
                </Pie>
                <Tooltip contentStyle={tooltipStyle} />
              </PieChart>
            </ResponsiveContainer>
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
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  data={groupData}
                  layout="vertical"
                  margin={{ left: 12, right: 24, top: 8, bottom: 8 }}
                >
                  <CartesianGrid horizontal={false} stroke="var(--chart-grid)" />
                  <XAxis
                    type="number"
                    axisLine={false}
                    tickLine={false}
                    tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
                  />
                  <YAxis
                    type="category"
                    dataKey="name"
                    axisLine={false}
                    tickLine={false}
                    tick={{ fill: 'var(--text-secondary)', fontSize: 11 }}
                    width={105}
                  />
                  <Tooltip contentStyle={tooltipStyle} cursor={{ fill: 'var(--hover)' }} />
                  <Bar dataKey="requests" fill="var(--chart-accent)" radius={[0, 4, 4, 0]} barSize={12}>
                    {groupData.map((group) => (
                      <Cell key={group.id} fill={group.color} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
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
                    <strong>{group.requests.toLocaleString('en-US')}</strong>
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
                {t('routePerformanceSubtitle')} · {t('localAnalyticsNote')}
              </p>
            </div>
            <span className="chart-unit">{periodLabel}</span>
          </div>
          <div className="group-performance-layout">
            <div
              className="group-performance-chart"
              style={{ height: Math.max(190, routeData.length * 52) }}
            >
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  data={routeData}
                  layout="vertical"
                  margin={{ left: 12, right: 24, top: 8, bottom: 8 }}
                >
                  <CartesianGrid horizontal={false} stroke="var(--chart-grid)" />
                  <XAxis
                    type="number"
                    axisLine={false}
                    tickLine={false}
                    tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
                  />
                  <YAxis
                    type="category"
                    dataKey="chartName"
                    axisLine={false}
                    tickLine={false}
                    tick={{ fill: 'var(--text-secondary)', fontSize: 11 }}
                    width={125}
                  />
                  <Tooltip contentStyle={tooltipStyle} cursor={{ fill: 'var(--hover)' }} />
                  <Bar dataKey="requests" fill="var(--chart-accent)" radius={[0, 4, 4, 0]} barSize={12}>
                    {routeData.map((route) => (
                      <Cell key={route.id} fill={route.color} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
            <div className="group-performance-list tabular">
              {routeData.map((route, index) => (
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
                    <strong>{route.requests.toLocaleString('en-US')}</strong>
                    <small>{route.share.toFixed(1)}%</small>
                  </div>
                  <div className="group-performance-health">
                    <span>
                      {route.success} {t('successShort')}
                    </span>
                    <span>{route.latency ? `${route.latency} ${t('avgShort')}` : '—'}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </section>
      </div>

      <section className="model-performance glass-panel">
        <div className="panel-heading">
          <div>
            <h3>{t('modelPerformance')}</h3>
            <p>
              {t('modelPerformanceSubtitle')} · {t('localAnalyticsNote')}
            </p>
          </div>
          <Layers3 size={17} />
        </div>
        <div className="model-bars">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={modelData} layout="vertical" margin={{ left: 15, right: 20 }}>
              <CartesianGrid horizontal={false} stroke="var(--chart-grid)" />
              <XAxis
                type="number"
                axisLine={false}
                tickLine={false}
                tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
              />
              <YAxis
                type="category"
                dataKey="model"
                axisLine={false}
                tickLine={false}
                tick={{ fill: 'var(--text-secondary)', fontSize: 11 }}
                width={100}
              />
              <Tooltip contentStyle={tooltipStyle} cursor={{ fill: 'var(--hover)' }} />
              <Bar dataKey="requests" fill="var(--chart-accent)" radius={[0, 4, 4, 0]} barSize={10} />
            </BarChart>
          </ResponsiveContainer>
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
