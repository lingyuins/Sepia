import { useEffect, useMemo, useState } from 'react'
import { Activity, ArrowUpRight, BarChart3, Blocks, ChevronRight, CircleHelp, Command, ExternalLink, Gauge, GitBranch, Inbox, Layers3, LayoutDashboard, Menu as MenuIcon, Plus, RefreshCw, Search, Settings, ShieldCheck, X } from 'lucide-react'
import { Dialog } from '@headlessui/react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { useWorkspace, useTranslation, type Provider, type RequestRecord } from './workspace'
import { gatewayUrl } from './gateway'
import { api, providerInput } from './api'
import { CopyButton, ProviderDialog, Toast } from './components/Provider'
import { Modal, ProviderCell, SectionHeading, StatusDot } from './components/UI'
import { Dropdown } from './components/Dropdown'
import { RouteChain } from './components/RouteChain'
import { latestSuccessfulRouteRequest, selectOverviewRoute } from './route-display'
import { ProvidersView } from './views/ProvidersView'
import { RoutingView } from './views/RoutingView'
import { PluginsView } from './views/PluginsView'
import { RequestsView, RequestsTable } from './views/RequestsView'
import { AnalyticsView } from './views/AnalyticsView'
import SettingsView from './components/SettingsView'
import { HelpDialog, WorkspaceDialog } from './components/ShellDialogs'

type Page = 'Overview' | 'Providers' | 'Routing' | 'Plugins' | 'Requests' | 'Analytics' | 'Settings'
type Period = '24 hours' | '7 days' | '30 days'
const navItems = [{ label: 'Overview', icon: LayoutDashboard }, { label: 'Providers', icon: Layers3 }, { label: 'Routing', icon: GitBranch }, { label: 'Plugins', icon: Blocks }] as const
const insightItems = [{ label: 'Requests', icon: Inbox }, { label: 'Analytics', icon: BarChart3 }] as const

function Logo({ compact = false }: { compact?: boolean }) {
  return <div className="brand"><span className="brand-symbol"><i /><i /><i /></span>{!compact && <span>sepia<span className="brand-dot">.</span></span>}</div>
}

function MetricCard({ title, value, foot, icon: Icon }: { title: string; value: string; foot: string; icon: typeof Activity }) {
  return <div className="metric-card"><div className="metric-top"><span>{title}</span><Icon size={16} /></div><div className="metric-value">{value}</div><div className="metric-foot">{foot}</div></div>
}

function Overview({ providers, navigate, onAdd, onNotify, onRequest }: { providers: Provider[]; navigate: (page: Page) => void; onAdd: () => void; onNotify: (message: string) => void; onRequest: (row: RequestRecord) => void }) {
  const { requests, routes, settings, analytics, refresh } = useWorkspace()
  const t = useTranslation()
  const endpoint = gatewayUrl(settings)
  const [period, setPeriod] = useState<Period>('30 days')
  const [selectedRouteId, setSelectedRouteId] = useState<number>()
  const defaultRoute = selectOverviewRoute(routes, requests)
  const selectedRoute = routes.find((route) => route.id === selectedRouteId && route.active) || defaultRoute
  useEffect(() => { if (selectedRouteId && !routes.some((route) => route.id === selectedRouteId && route.active)) setSelectedRouteId(undefined) }, [routes, selectedRouteId])
  const latestRequest = selectedRoute ? latestSuccessfulRouteRequest(selectedRoute, requests) : undefined
  const data = analytics.series || []
  const periodLabel = period === '24 hours' ? t('24h') : period === '7 days' ? t('7d') : t('30d')
  const [refreshing, setRefreshing] = useState(false)
  const refreshData = async () => { setRefreshing(true); try { await refresh(period); onNotify(t('metricsRefreshed')) } finally { setRefreshing(false) } }
  return <>
    <SectionHeading title={t('overview')} description={t('overviewSubtitle')} action={<><button className="button secondary" onClick={refreshData}><RefreshCw size={15} className={refreshing ? 'spin' : ''} />{t('refresh')}</button><button className="button primary" onClick={onAdd}><Plus size={15} />{t('addProvider')}</button></>} />
    <section className="connection-band"><StatusDot /><strong>{t('gatewayOperational')}</strong><span>{settings.workspace} · {providers.filter((provider) => provider.enabled).length} {t('providersEnabledShort')}</span><div className="connection-endpoint"><span>{t('configuredEndpoint')}</span><code>{endpoint}</code><CopyButton value={endpoint} onCopy={() => onNotify(t('endpointCopied'))} /></div></section>
    <section className="route-chain-panel glass-panel"><div className="panel-heading"><div><h2>{t('routeChain')}</h2><p>{t('routePreviewHint')}</p></div><div className="route-chain-actions">{selectedRoute && <Dropdown variant="compact" label={t('selectRoutePreview')} value={String(selectedRoute.id)} options={routes.filter((route) => route.active).map((route) => ({ value: String(route.id), label: route.name || route.slug || `Route ${route.id}` }))} onChange={(value) => setSelectedRouteId(Number(value))} />}<button className="button text-button" onClick={() => navigate('Routing')}>{t('manageRoute')}<ArrowUpRight size={14} /></button></div></div>
      {selectedRoute ? <><RouteChain route={selectedRoute} latestRequest={latestRequest} compact />{!latestRequest && <p className="route-empty-note">{t('noSuccessfulRouteRequest')}</p>}</> : <div className="route-chain-empty"><GitBranch size={24} /><strong>{t('noEnabledRoutes')}</strong><p>{t('createRoutePrompt')}</p><button className="button secondary" onClick={() => navigate('Routing')}>{t('createRoute')}</button></div>}
    </section>
    <div className="metrics-grid">
      <MetricCard title={t('requestsInPeriod')} value={analytics.requests.toLocaleString()} foot={periodLabel} icon={Inbox} />
      <MetricCard title={t('successRate')} value={analytics.requests ? `${(analytics.successful / analytics.requests * 100).toFixed(2)}%` : '—'} foot={`${analytics.successful.toLocaleString()} ${t('successful')}`} icon={ShieldCheck} />
      <MetricCard title={t('periodLatency')} value={analytics.latency_ms ? `${Math.round(analytics.latency_ms)} ms` : '—'} foot={t('noLatencyData')} icon={Gauge} />
      <MetricCard title={t('periodCost')} value={`$${(analytics.cost_micros / 1000000).toFixed(4)}`} foot={periodLabel} icon={BarChart3} />
    </div>
    <div className="dashboard-grid"><section className="chart-panel glass-panel"><div className="panel-heading"><div><h3>{t('requestActivity')}</h3><p>{t('trafficSubtitle')}</p></div><div className="segmented">{(['24 hours', '7 days', '30 days'] as Period[]).map((item) => <button className={period === item ? 'active' : ''} onClick={() => { setPeriod(item); void refresh(item) }} key={item}>{item === '24 hours' ? t('24h') : item === '7 days' ? t('7d') : t('30d')}</button>)}</div></div><div className="chart-legend"><span><i className="legend-mint" />{t('allRequests')} <strong>{analytics.requests.toLocaleString()}</strong></span><span><i className="legend-grey" />{t('successful')} <strong>{analytics.successful.toLocaleString()}</strong></span></div><div className="traffic-chart"><ResponsiveContainer width="100%" height="100%"><AreaChart data={data} margin={{ top: 14, right: 4, left: -24, bottom: 0 }}><defs><linearGradient id="requestGradient" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--chart-accent)" stopOpacity={0.18} /><stop offset="100%" stopColor="var(--chart-accent)" stopOpacity={0} /></linearGradient></defs><CartesianGrid vertical={false} stroke="var(--chart-grid)" /><XAxis dataKey="time" axisLine={false} tickLine={false} tick={{ fill: 'var(--chart-text)', fontSize: 12 }} interval={2} dy={10} /><YAxis axisLine={false} tickLine={false} tick={{ fill: 'var(--chart-text)', fontSize: 12 }} allowDecimals={false} width={44} /><Tooltip contentStyle={{ background: 'var(--panel-solid)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--text)' }} /><Area type="monotone" dataKey="requests" stroke="var(--chart-accent)" strokeWidth={2} fill="url(#requestGradient)" /><Area type="monotone" dataKey="successful" stroke="var(--chart-series-2)" strokeDasharray="4 4" fill="transparent" /></AreaChart></ResponsiveContainer></div></section>
      <section className="health-panel glass-panel"><div className="panel-heading"><div><h3>{t('providerHealth')}</h3><p>{t('providerHealthSubtitle')}</p></div><button className="icon-button" onClick={() => navigate('Providers')} aria-label={t('viewProviders')}><ArrowUpRight size={17} /></button></div><div className="health-list">{providers.slice(0, 4).map((provider) => <div className="health-row" key={provider.id}><ProviderCell name={provider.name} /><div className="health-latency"><strong>{provider.latency ? `${provider.latency} ms` : '—'}</strong><span className={provider.enabled ? 'positive' : 'muted'}><StatusDot />{provider.enabled ? t('operational') : t('paused')}</span></div></div>)}{!providers.length && <div className="empty-state">{t('noProvidersYet')}</div>}</div><button className="add-provider-link" onClick={onAdd}><Plus size={14} />{t('connectAnother')}</button></section></div>
    <section className="recent-panel glass-panel"><div className="panel-heading"><div><h3>{t('recentRequests')}</h3><p>{t('recentRequestsSubtitle')}</p></div><button className="button text-button" onClick={() => navigate('Requests')}>{t('viewAllRequests')}<ArrowUpRight size={14} /></button></div><RequestsTable compact rows={requests.slice(0, 5)} onSelect={onRequest} /></section>
  </>
}

function App() {
  const { settings, providers, plugins, routes, requests, notify, toast, clearToast, run } = useWorkspace()
  const t = useTranslation()
  const getPage = (): Page => { const p = window.location.hash.slice(1); return p === 'Settings' || [...navItems, ...insightItems].some((item) => item.label === p) ? p as Page : 'Overview' }
  const [page, setPage] = useState<Page>(getPage)
  const [mobileNav, setMobileNav] = useState(false)
  const [addProvider, setAddProvider] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [helpOpen, setHelpOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [selectedRequest, setSelectedRequest] = useState<RequestRecord | null>(null)
  const [workspaceOpen, setWorkspaceOpen] = useState(false)
  const navigate = (next: Page) => { setPage(next); window.location.hash = next; setMobileNav(false); window.scrollTo({ top: 0, behavior: settings.reducedMotion ? 'auto' : 'smooth' }) }
  useEffect(() => { const listener = () => setPage(getPage()); window.addEventListener('hashchange', listener); return () => window.removeEventListener('hashchange', listener) }, [])
  useEffect(() => { const handler = (event: KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.key === 'k') { event.preventDefault(); setSearchOpen(true) } }; window.addEventListener('keydown', handler); return () => window.removeEventListener('keydown', handler) }, [])
  const filteredPages: { label: Page; icon: typeof Settings }[] = [...navItems, ...insightItems, { label: 'Settings' as const, icon: Settings }]
  const entityItems = useMemo(() => [...providers.map((item) => ({ label: item.name, type: 'Provider', page: 'Providers' as Page, icon: Layers3 })), ...routes.map((item) => ({ label: item.name, type: 'Route', page: 'Routing' as Page, icon: GitBranch })), ...plugins.map((item) => ({ label: item.name, type: 'Plugin', page: 'Plugins' as Page, icon: Blocks })), ...requests.map((item) => ({ label: item.model, type: 'Request', page: 'Requests' as Page, icon: Inbox }))], [providers, routes, plugins, requests])
  const searchResults = [...filteredPages.map((item) => ({ ...item, type: 'Page', page: item.label })), ...entityItems].filter((item) => `${item.label} ${item.type}`.toLowerCase().includes(search.toLowerCase())).slice(0, 12)
  return <div className="app-shell">
    {mobileNav && <button className="mobile-overlay" onClick={() => setMobileNav(false)} aria-label={t('closeNavigation')} />}
    <aside className={`sidebar ${mobileNav ? 'mobile-open' : ''}`}><div className="sidebar-top"><Logo /><button className="mobile-close icon-button" onClick={() => setMobileNav(false)} aria-label={t('closeNavigation')}><X size={18} /></button></div><div className="sidebar-workspace"><StatusDot /><div><strong>{settings.workspace}</strong><span>{t('browserWorkspace')}</span></div></div><nav className="sidebar-nav"><span className="nav-label">{t('workspace')}</span>{navItems.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${page === label ? 'active' : ''}`} onClick={() => navigate(label)}><Icon size={17} /><span>{t(label.toLowerCase() as 'overview')}</span>{label === 'Providers' && <span className="nav-count">{providers.length}</span>}{label === 'Plugins' && <span className="nav-count">{plugins.length}</span>}</button>)}<span className="nav-label insights-label">{t('insights')}</span>{insightItems.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${page === label ? 'active' : ''}`} onClick={() => navigate(label)}><Icon size={17} /><span>{t(label.toLowerCase() as 'overview')}</span>{label === 'Requests' && <span className="nav-live-dot" />}</button>)}</nav><div className="sidebar-bottom"><button className="nav-item" onClick={() => setHelpOpen(true)}><CircleHelp size={17} /><span>{t('helpDocs')}</span><ExternalLink size={12} /></button><button className={`nav-item ${page === 'Settings' ? 'active' : ''}`} onClick={() => navigate('Settings')}><Settings size={17} /><span>{t('settings')}</span></button><div className="gateway-card"><span className="live-pill"><StatusDot />{t('gatewayOperational')}</span><code>{settings.host}:{settings.port}</code><small>{providers.filter((provider) => provider.enabled).length} / {providers.length} {t('providersEnabledShort')}</small></div></div></aside>
    <main className="main-content"><header className="topbar"><div className="topbar-left"><button className="mobile-menu icon-button" onClick={() => setMobileNav(true)} aria-label={t('openNavigation')}><MenuIcon size={20} /></button><span className="breadcrumb">{t('workspace')}<ChevronRight size={12} /><strong>{t(page.toLowerCase() as 'overview')}</strong></span></div><div className="topbar-actions"><button className="top-search" onClick={() => setSearchOpen(true)}><Search size={16} /><span>{t('searchAnything')}</span><kbd><Command size={11} /> K</kbd></button><button className="top-avatar" onClick={() => setWorkspaceOpen(true)} aria-label={t('workspaceProfile')}>{settings.workspace.charAt(0)}</button></div></header><div className="page-content">
      {page === 'Overview' && <Overview providers={providers} navigate={navigate} onAdd={() => setAddProvider(true)} onNotify={notify} onRequest={setSelectedRequest} />}
      {page === 'Providers' && <ProvidersView />}{page === 'Routing' && <RoutingView />}{page === 'Plugins' && <PluginsView />}{page === 'Requests' && <RequestsView onSelect={setSelectedRequest} />}{page === 'Analytics' && <AnalyticsView />}{page === 'Settings' && <SettingsView />}
      <footer className="page-footer"><span><Logo compact />{t('footer')}</span><span>Sepia v0.1.0</span></footer>
    </div></main>
    <ProviderDialog open={addProvider} onClose={() => setAddProvider(false)} onSave={async (provider, secrets) => { await run(() => api('/api/v1/providers', { method: 'POST', body: JSON.stringify({ ...providerInput(provider), secrets }) })); setAddProvider(false); notify(t('providerConnected', { name: provider.name })) }} />
    <Dialog open={searchOpen} onClose={() => setSearchOpen(false)} className="dialog-root"><div className="dialog-backdrop" /><div className="dialog-position search-position"><Dialog.Panel className="dialog-panel search-dialog glass-panel"><div className="search-dialog-input"><Search size={19} /><input autoFocus placeholder={t('searchPlaceholder')} aria-label={t('searchWorkspace')} value={search} onChange={(event) => setSearch(event.target.value)} /><button className="icon-button" onClick={() => setSearchOpen(false)} aria-label={t('close')}><X size={16} /></button></div><div className="search-results"><span className="nav-label">{t('page')}</span>{searchResults.map(({ label, icon: Icon, type, page: target }, index) => <button key={`${type}-${label}-${index}`} onClick={() => { navigate(target); setSearchOpen(false); setSearch('') }}><Icon size={17} /><span>{label}</span><small>{type}</small><ArrowUpRight size={14} /></button>)}{!searchResults.length && <p>{t('searchEmpty')}</p>}</div><div className="search-footer"><span><kbd>↵</kbd> {t('toSelect')}</span><span><kbd>esc</kbd> {t('toClose')}</span></div></Dialog.Panel></div></Dialog>
    <Modal open={!!selectedRequest} onClose={() => setSelectedRequest(null)} title={t('requestDetails')} description={t('requestDetailsSubtitle')}>{selectedRequest && <><dl className="request-details"><div><dt>{t('requestId')}</dt><dd>{selectedRequest.id}</dd></div><div><dt>{t('time')}</dt><dd>{selectedRequest.time}</dd></div><div><dt>{t('model')}</dt><dd>{selectedRequest.model}</dd></div><div><dt>{t('provider')}</dt><dd><ProviderCell name={selectedRequest.provider} /></dd></div><div><dt>{t('route')}</dt><dd>{selectedRequest.route}</dd></div><div><dt>{t('apiKey')}</dt><dd>{selectedRequest.key}</dd></div><div><dt>{t('tokensDetail')}</dt><dd>{t('inputOutput', { input: selectedRequest.inputTokens.toLocaleString(), output: selectedRequest.outputTokens.toLocaleString() })}</dd></div><div><dt>{t('latency')}</dt><dd>{selectedRequest.latency}</dd></div><div><dt>{t('estimatedCost')}</dt><dd>{selectedRequest.cost}</dd></div><div><dt>{t('status')}</dt><dd><span className="status-pill"><StatusDot />{selectedRequest.status}</span></dd></div></dl><div className="secure-note"><ShieldCheck size={15} />{t('metadataOnly')}</div></>}</Modal>
    <HelpDialog open={helpOpen} onClose={() => setHelpOpen(false)} /><WorkspaceDialog open={workspaceOpen} onClose={() => setWorkspaceOpen(false)} onSettings={() => navigate('Settings')} />{toast && <Toast message={toast} onClose={clearToast} />}
  </div>
}
export default App
