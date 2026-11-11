import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import {
  Activity, ArrowRight, ArrowUpRight, BarChart3, Bell, Blocks,
  ChevronRight, CircleHelp, Command,
  ExternalLink, Gauge, GitBranch, Inbox, Layers3, LayoutDashboard,
  Menu as MenuIcon, Network, Plus, RefreshCw,
  Search, Settings, ShieldCheck, Sparkles, TerminalSquare, X, Zap,
} from 'lucide-react'
import { Dialog } from '@headlessui/react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { useWorkspace, useTranslation, type Provider, type RequestRecord } from './workspace'
import { gatewayUrl } from './gateway'
import { api, providerInput } from './api'
import { CopyButton, ProviderDialog, ProviderMark, Toast } from './components/Provider'
import { ProvidersView } from './views/ProvidersView'
import { RoutingView } from './views/RoutingView'
import { PluginsView } from './views/PluginsView'
import { RequestsView, RequestsTable } from './views/RequestsView'
import { AnalyticsView } from './views/AnalyticsView'
import SettingsView from './components/SettingsView'
import { HelpDialog, NotificationsPanel, WorkspaceDialog } from './components/ShellDialogs'

const RoutingOrb = lazy(() => import('./components/RoutingOrb'))
type Page = 'Overview' | 'Providers' | 'Routing' | 'Plugins' | 'Requests' | 'Analytics' | 'Settings'
type Period = '24 hours' | '7 days' | '30 days'

const navItems = [
  { label: 'Overview', icon: LayoutDashboard }, { label: 'Providers', icon: Layers3 },
  { label: 'Routing', icon: GitBranch }, { label: 'Plugins', icon: Blocks },
] as const
const insightItems = [{ label: 'Requests', icon: Inbox }, { label: 'Analytics', icon: BarChart3 }] as const

function Logo({ compact = false }: { compact?: boolean }) {
  return <div className="brand"><span className="brand-symbol"><i /><i /><i /></span>{!compact && <span>sepia<span className="brand-dot">.</span></span>}</div>
}

function StatusDot() { return <span className="status-dot" /> }

function ProviderCell({ name }: { name: string }) {
  const { providers } = useWorkspace()
  const provider = providers.find((p) => p.name === name)
  return <span className="provider-cell">{provider ? <ProviderMark {...provider} small /> : <span className="provider-mark small">{name.charAt(0)}</span>}{name}</span>
}

function SectionHeading({ eyebrow, title, description, action }: { eyebrow?: string; title: string; description: string; action?: React.ReactNode }) {
  return <div className="page-heading"><div>{eyebrow && <div className="eyebrow">{eyebrow}</div>}<h1>{title}</h1><p>{description}</p></div>{action}</div>
}

function MetricCard({ title, value, foot, icon: Icon }: { title: string; value: string; foot: string; icon: typeof Activity }) {
  return <div className="metric-card glass-panel">
    <div className="metric-top"><span>{title}</span><span className="metric-icon"><Icon size={16} /></span></div>
    <div className="metric-value">{value}</div>
    <div className="metric-foot"><span>{foot}</span></div>
  </div>
}

function Overview({ providers, navigate, onAdd, onNotify, onRequest }: { providers: Provider[]; navigate: (page: Page) => void; onAdd: () => void; onNotify: (message: string) => void; onRequest: (row: RequestRecord) => void }) {
  const { requests, settings, analytics, refresh } = useWorkspace()
  const t = useTranslation()
  const endpoint = gatewayUrl(settings)
  const [period, setPeriod] = useState<Period>('30 days')
  const periodLabel = (value: Period) => value === '24 hours' ? '24h' : value === '7 days' ? '7d' : '30d'
  const data = analytics.series || []
  const [refreshing, setRefreshing] = useState(false)
  const refreshData = async () => { setRefreshing(true); try { await refresh(period); onNotify(t('metricsRefreshed')) } finally { setRefreshing(false) } }
  return <>
    <SectionHeading title={t('goodToSeeYou')} description={t('overviewSubtitle')} action={<button className="button secondary heading-refresh" onClick={refreshData}><RefreshCw size={15} className={refreshing ? 'spin' : ''} />{t('refresh')}</button>} />
    <section className="hero-panel glass-panel">
      <div className="hero-copy"><div className="hero-badge"><Sparkles size={12} /> {t('heroEyebrow')}</div><h2>{t('heroTitle')}<span>.</span></h2><p>{t('heroSubtitle')}</p><div className="hero-actions"><button className="button primary" onClick={() => navigate('Routing')}>{t('exploreRouting')} <ArrowUpRight size={16} /></button><button className="button text-button" onClick={() => navigate('Providers')}>{t('viewProviders')} <ArrowRight size={15} /></button></div></div>
      <div className="orb-wrap"><div className="orb-halo" />{!settings.reducedMotion && <Suspense fallback={<div className="orb-fallback" />}><RoutingOrb /></Suspense>}<div className="orb-label label-one"><StatusDot />{providers.filter((p) => p.enabled).length} {t('providersEnabledShort')}</div><div className="orb-label label-two"><Zap size={11} /> {t('gatewayReady')}</div></div>
      <div className="hero-grid" />
    </section>
    <div className="metrics-grid">
      <MetricCard title={t('requestsToday')} value={analytics.requests.toLocaleString()} foot={periodLabel(period)} icon={Inbox} />
      <MetricCard title={t('successRate')} value={analytics.requests ? `${(analytics.successful / analytics.requests * 100).toFixed(2)}%` : '—'} foot={periodLabel(period)} icon={ShieldCheck} />
      <MetricCard title={t('avgLatency')} value={analytics.latency_ms ? `${Math.round(analytics.latency_ms)} ms` : '—'} foot={periodLabel(period)} icon={Gauge} />
      <MetricCard title={t('spendToday')} value={`$${(analytics.cost_micros / 1000000).toFixed(4)}`} foot={periodLabel(period)} icon={BarChart3} />
    </div>
    <div className="dashboard-grid">
      <section className="chart-panel glass-panel">
        <div className="panel-heading"><div><h3>{t('requestActivity')}</h3><p>{t('trafficSubtitle')}</p></div><div className="segmented">{(['24 hours', '7 days', '30 days'] as Period[]).map((p) => <button className={period === p ? 'active' : ''} onClick={() => { setPeriod(p); void refresh(p) }} key={p}>{p === '24 hours' ? '24h' : p === '7 days' ? '7d' : '30d'}</button>)}</div></div>
        <div className="chart-legend"><span><i className="legend-mint" />{t('allRequests')} <strong>{analytics.requests.toLocaleString()}</strong></span><span><i className="legend-grey" />{t('successful')} <strong>{analytics.successful.toLocaleString()}</strong></span></div>
        <div className="traffic-chart"><ResponsiveContainer width="100%" height="100%"><AreaChart data={data} margin={{ top: 14, right: 4, left: -24, bottom: 0 }}><defs><linearGradient id="requestGradient" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--chart-accent)" stopOpacity={0.22} /><stop offset="100%" stopColor="var(--chart-accent)" stopOpacity={0} /></linearGradient></defs><CartesianGrid vertical={false} stroke="var(--chart-grid)" /><XAxis dataKey="time" axisLine={false} tickLine={false} tick={{ fill: 'var(--chart-text)', fontSize: 10 }} interval={2} dy={10} /><YAxis axisLine={false} tickLine={false} tick={{ fill: 'var(--chart-text)', fontSize: 10 }} ticks={[0, 250, 500, 750, 1000, 1250]} /><Tooltip contentStyle={{ background: 'var(--panel-solid)', border: '1px solid var(--border)', borderRadius: 10, fontSize: 12 }} labelStyle={{ color: 'var(--chart-text)' }} cursor={{ stroke: 'var(--chart-accent)' }} /><Area type="monotone" dataKey="requests" stroke="var(--chart-accent)" strokeWidth={2} fill="url(#requestGradient)" activeDot={{ r: 4, fill: 'var(--chart-accent)', stroke: 'var(--panel-solid)', strokeWidth: 3 }} /><Area type="monotone" dataKey="successful" stroke="var(--chart-text)" strokeWidth={1} strokeDasharray="4 5" fill="transparent" /></AreaChart></ResponsiveContainer></div>
      </section>
      <section className="health-panel glass-panel"><div className="panel-heading"><div><h3>{t('providerHealth')}</h3><p>{t('providerHealthSubtitle')}</p></div><button className="icon-button" onClick={() => navigate('Providers')} aria-label="{t('viewProviders')}"><ArrowUpRight size={17} /></button></div><div className="health-list">{providers.slice(0, 4).map((provider) => <div className="health-row" key={provider.id}><ProviderMark {...provider} /><div className="health-name"><strong>{provider.name}</strong><span>{provider.models} {t('modelsConnected')}</span></div><div className="health-latency"><strong>{provider.latency ? `${provider.latency} ms` : t('new')}</strong><span><StatusDot />{provider.enabled ? t('operational') : t('paused')}</span></div></div>)}</div><button className="add-provider-link" onClick={onAdd}><Plus size={14} /> {t('connectAnother')}</button></section>
    </div>
    <section className="recent-panel glass-panel"><div className="panel-heading"><div><h3>{t('recentRequests')}</h3><p>{t('recentRequestsSubtitle')}</p></div><button className="button text-button" onClick={() => navigate('Requests')}>{t('viewAllRequests')} <ArrowUpRight size={14} /></button></div><RequestsTable compact rows={requests.slice(0, 5)} onSelect={onRequest} /></section>
    <div className="endpoint-bar"><span className="endpoint-icon"><TerminalSquare size={15} /></span><span>{t('gatewayEndpoint')}</span><code>{endpoint}</code><CopyButton value={endpoint} onCopy={() => onNotify(t('endpointCopied'))} /></div>
  </>
}





function App() {
  const { settings, providers, plugins, routes, requests, notices, notify, toast, clearToast, run } = useWorkspace()
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
  const [notificationsOpen, setNotificationsOpen] = useState(false)
  const notificationRef = useRef<HTMLDivElement>(null)
  const navigate = (next: Page) => { setPage(next); window.location.hash = next; setMobileNav(false); window.scrollTo({ top: 0, behavior: 'smooth' }) }
  useEffect(() => { const listener = () => setPage(getPage()); window.addEventListener('hashchange', listener); return () => window.removeEventListener('hashchange', listener) }, [])
  useEffect(() => { const close = (event: MouseEvent) => { if (notificationRef.current && !notificationRef.current.contains(event.target as Node)) setNotificationsOpen(false) }; if (notificationsOpen) document.addEventListener('mousedown', close); return () => document.removeEventListener('mousedown', close) }, [notificationsOpen])
  useEffect(() => { const handler = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key === 'k') { e.preventDefault(); setSearchOpen(true) } }; window.addEventListener('keydown', handler); return () => window.removeEventListener('keydown', handler) }, [])
  const filteredPages: { label: Page; icon: typeof Settings }[] = [...navItems, ...insightItems, { label: 'Settings' as const, icon: Settings }]
  const entityItems = useMemo(() => [
    ...providers.map((item) => ({ label: item.name, type: 'Provider', page: 'Providers' as Page, icon: Layers3 })),
    ...routes.map((item) => ({ label: item.name, type: 'Route', page: 'Routing' as Page, icon: GitBranch })),
    ...plugins.map((item) => ({ label: item.name, type: 'Plugin', page: 'Plugins' as Page, icon: Blocks })),
    ...requests.map((item) => ({ label: item.model, type: 'Request', page: 'Requests' as Page, icon: Inbox })),
  ], [providers, routes, plugins, requests])
  const searchResults = [...filteredPages.map((item) => ({ ...item, type: 'Page', page: item.label })), ...entityItems].filter((item) => `${item.label} ${item.type}`.toLowerCase().includes(search.toLowerCase())).slice(0, 12)

  return <div className="app-shell">
    {mobileNav && <button className="mobile-overlay" onClick={() => setMobileNav(false)} aria-label={t('closeNavigation')} />}
    <aside className={`sidebar ${mobileNav ? 'mobile-open' : ''}`}>
      <div className="sidebar-top"><Logo /><button className="mobile-close icon-button" onClick={() => setMobileNav(false)}><X size={18} /></button></div>
      <nav className="sidebar-nav"><span className="nav-label">{t('workspace')}</span>{navItems.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${page === label ? 'active' : ''}`} onClick={() => navigate(label)}><Icon size={17} strokeWidth={1.65} /><span>{t(label.toLowerCase() as 'overview') || label}</span>{label === 'Providers' && <span className="nav-count">{providers.length}</span>}{label === 'Plugins' && <span className="nav-count">{plugins.length}</span>}</button>)}<span className="nav-label insights-label">{t('insights')}</span>{insightItems.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${page === label ? 'active' : ''}`} onClick={() => navigate(label)}><Icon size={17} strokeWidth={1.65} /><span>{t(label.toLowerCase() as 'overview') || label}</span>{label === 'Requests' && <span className="nav-live-dot" />}</button>)}</nav>
      <div className="sidebar-bottom"><div className="gateway-card"><div className="gateway-card-top"><span className="gateway-icon"><Network size={15} /></span><span className="live-pill"><StatusDot />{t('preview')}</span></div><strong>{t('yourWorkspace')}</strong><span>{settings.host}:{settings.port}</span><div className="uptime-track"><i /></div><small><span>{t('providersEnabled')}</span><span>{providers.filter((p) => p.enabled).length} / {providers.length}</span></small></div><button className="nav-item" onClick={() => setHelpOpen(true)}><CircleHelp size={17} /><span>{t('helpDocs')}</span><ExternalLink size={12} className="help-icon" /></button><button className={`nav-item ${page === 'Settings' ? 'active' : ''}`} onClick={() => navigate('Settings')}><Settings size={17} /><span>{t('settings')}</span></button></div>
    </aside>
    <main className="main-content"><header className="topbar"><div className="topbar-left"><button className="mobile-menu icon-button" onClick={() => setMobileNav(true)} aria-label={t('openNavigation')}><MenuIcon size={20} /></button><span className="breadcrumb">{t('workspace')} <ChevronRight size={12} /><strong>{t(page.toLowerCase() as 'overview')}</strong></span></div><div className="topbar-actions"><button className="top-search" onClick={() => setSearchOpen(true)}><Search size={15} /><span>{t('searchAnything')}</span><kbd><Command size={10} /> K</kbd></button><span className="topbar-divider" /><div className="notification-anchor" ref={notificationRef}><button className="icon-button notification-button" onClick={() => setNotificationsOpen(!notificationsOpen)} aria-label={t('notifications')}><Bell size={18} />{notices.some((n) => !n.read) && <i />}</button><NotificationsPanel open={notificationsOpen} onClose={() => setNotificationsOpen(false)} onRequests={() => navigate('Requests')} /></div><button className="top-avatar" onClick={() => setWorkspaceOpen(true)} aria-label={t('workspaceProfile')}>{settings.workspace.charAt(0)}</button></div></header><div className="page-content">
      {page === 'Overview' && <Overview providers={providers} navigate={navigate} onAdd={() => setAddProvider(true)} onNotify={notify} onRequest={setSelectedRequest} />}
      {page === 'Providers' && <ProvidersView />}
      {page === 'Routing' && <RoutingView />}
      {page === 'Plugins' && <PluginsView />}
      {page === 'Requests' && <RequestsView onSelect={setSelectedRequest} />}
      {page === 'Analytics' && <AnalyticsView />}
      {page === 'Settings' && <SettingsView />}
      <footer className="page-footer"><span><Logo compact />{t('footer')}</span><span>Sepia v0.1.0 <i /> <span className="positive">{t('frontendPreview')}</span></span></footer>
    </div></main>
    <ProviderDialog open={addProvider} onClose={() => setAddProvider(false)} onSave={async (provider, secrets) => { await run(() => api('/api/v1/providers', { method: 'POST', body: JSON.stringify({ ...providerInput(provider), secrets }) })); setAddProvider(false); notify(t('providerConnected', { name: provider.name })) }} />
    <Dialog open={searchOpen} onClose={() => setSearchOpen(false)} className="dialog-root"><div className="dialog-backdrop" /><div className="dialog-position search-position"><Dialog.Panel className="dialog-panel search-dialog glass-panel"><div className="search-dialog-input"><Search size={19} /><input autoFocus placeholder={t('searchPlaceholder')} aria-label={t('searchWorkspace')} value={search} onChange={(e) => setSearch(e.target.value)} /><button className="icon-button" onClick={() => setSearchOpen(false)}><X size={16} /></button></div><div className="search-results"><span className="nav-label">{t('page')}</span>{searchResults.map(({ label, icon: Icon, type, page: target }, index) => <button key={`${type}-${label}-${index}`} onClick={() => { navigate(target); setSearchOpen(false); setSearch('') }}><Icon size={17} /><span>{label}</span><small>{type}</small><ArrowUpRight size={14} /></button>)}{searchResults.length === 0 && <p>{t('searchEmpty')}</p>}</div><div className="search-footer"><span><kbd>↵</kbd> {t('toSelect')}</span><span><kbd>esc</kbd> {t('toClose')}</span></div></Dialog.Panel></div></Dialog>
    {selectedRequest && <Dialog open={!!selectedRequest} onClose={() => setSelectedRequest(null)} className="dialog-root"><div className="dialog-backdrop" /><div className="dialog-position"><Dialog.Panel className="dialog-panel glass-panel"><button className="icon-button dialog-close" onClick={() => setSelectedRequest(null)} aria-label={t('close')}><X size={18} /></button><div className="dialog-symbol"><Activity size={22} /></div><Dialog.Title>{t('requestDetails')}</Dialog.Title><Dialog.Description>{t('requestDetailsSubtitle')}</Dialog.Description><dl className="request-details"><div><dt>{t('requestId')}</dt><dd>{selectedRequest.id}</dd></div><div><dt>{t('time')}</dt><dd>{selectedRequest.time}</dd></div><div><dt>{t('model')}</dt><dd>{selectedRequest.model}</dd></div><div><dt>{t('provider')}</dt><dd><ProviderCell name={selectedRequest.provider} /></dd></div><div><dt>{t('route')}</dt><dd>{selectedRequest.route}</dd></div><div><dt>{t('apiKey')}</dt><dd>{selectedRequest.key === 'Development' ? t('development') : selectedRequest.key === 'Production' ? t('production') : selectedRequest.key}</dd></div><div><dt>{t('tokensDetail')}</dt><dd>{t('inputOutput', { input: selectedRequest.inputTokens.toLocaleString(), output: selectedRequest.outputTokens.toLocaleString() })}</dd></div><div><dt>{t('latency')}</dt><dd>{selectedRequest.latency}</dd></div><div><dt>{t('estimatedCost')}</dt><dd>{selectedRequest.cost}</dd></div><div><dt>{t('status')}</dt><dd><span className={`status-pill ${selectedRequest.status === 'Failover' ? 'warning' : ''}`}><StatusDot />{selectedRequest.status === 'Failover' ? t('failover') : selectedRequest.status}</span></dd></div></dl><div className="secure-note"><ShieldCheck size={15} />{t('metadataOnly')}</div></Dialog.Panel></div></Dialog>}
    <HelpDialog open={helpOpen} onClose={() => setHelpOpen(false)} />
    <WorkspaceDialog open={workspaceOpen} onClose={() => setWorkspaceOpen(false)} onSettings={() => navigate('Settings')} />
    {toast && <Toast message={toast} onClose={clearToast} />}
  </div>
}

export default App
