import { useState } from 'react'
import { ArrowUpRight, Globe2, Pencil, Plus, Trash2 } from 'lucide-react'
import { useWorkspace, useTranslation, type Provider } from '../workspace'
import { api, providerInput } from '../api'
import { ProviderDialog, ProviderMark } from '../components/Provider'
import { DialogActions, Modal, SectionHeading, StatusDot } from '../components/UI'
import { Dropdown } from '../components/Dropdown'

export function ProvidersView() {
  const { providers, notify, routes, channelGroups, run } = useWorkspace()
  const groupLabel = (id: string) => id === 'official' ? t('officialChannels') : id === 'free' ? t('freeChannels') : id === 'team' ? t('teamChannels') : channelGroups.find((item) => item.id === id)?.name || t('noGroup')
  const protocolLabel = (type: string) => type === 'anthropic' ? t('anthropicCompatible') : type === 'gemini' ? t('geminiCompatible') : t('openaiCompatible')
  const t = useTranslation()
  const [editing, setEditing] = useState<Provider | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [deleting, setDeleting] = useState<Provider | null>(null)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('All')
  const [groupFilter, setGroupFilter] = useState('all')
  const filtered = providers.filter((p) => p.name.toLowerCase().includes(query.toLowerCase()) && (filter === 'All' || (filter === 'Active' ? p.enabled : !p.enabled)) && (groupFilter === 'all' || p.group === groupFilter))
  const save = async (provider: Provider, secrets: string[]) => {
    const existing = providers.some((item) => item.id === provider.id)
    const input = providerInput(provider, secrets)
    await run(() => api(existing ? `/api/v1/providers/${provider.id}` : '/api/v1/providers', { method: existing ? 'PUT' : 'POST', body: JSON.stringify(input) }))
    if (existing && secrets.length) await run(() => Promise.all(secrets.map((secret) => api(`/api/v1/providers/${provider.id}/keys`, { method: 'POST', body: JSON.stringify({ secret }) }))))
    setDialogOpen(false); setEditing(null); notify(editing ? t('providerUpdated', { name: provider.name }) : t('providerConnected', { name: provider.name }))
  }
  const remove = async () => {
    if (!deleting) return
    await run(() => api(`/api/v1/providers/${deleting.id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled: false }) }))
    notify(t('providerDisconnected', { name: deleting.name })); setDeleting(null)
  }
  return <>
    <SectionHeading eyebrow={t('infrastructure')} title={t('providersTitle')} description={t('providersSubtitle')} action={<button className="button primary" onClick={() => { setEditing(null); setDialogOpen(true) }}><Plus size={16} />{t('addProvider')}</button>} />
    <div className="provider-overview glass-panel"><Globe2 size={20} /><div><strong>{providers.filter((p) => p.enabled).length} {t('providersConnected')}</strong><span>{providers.reduce((sum, p) => sum + p.models, 0)} {t('modelsAvailable')}</span></div><span className="live-pill"><StatusDot />{providers.some((p) => p.enabled) ? t('readyNext') : providers.length ? t('gatewayPaused') : t('noProvidersConfigured')}</span></div>
    <div className="list-toolbar provider-toolbar"><div className="search-field"><input aria-label={t('findProvider')} placeholder={t('findProvider')} value={query} onChange={(e) => setQuery(e.target.value)} /></div><div className="filter-tabs">{['All', 'Active', t('paused')].map((item) => <button key={item} className={filter === item ? 'active' : ''} onClick={() => setFilter(item)}>{item === 'All' ? t('all') : item === 'Active' ? t('active') : t('pausedFilter')}</button>)}</div><Dropdown className="filter-select" variant="compact" label={t('allGroups')} value={groupFilter} options={[{ value: 'all', label: t('allGroups') }, ...channelGroups.map((item) => ({ value: item.id, label: groupLabel(item.id) }))]} onChange={setGroupFilter} /></div>
    {filtered.length ? <div className="providers-grid">{filtered.map((provider) => <div className={`provider-card glass-panel ${!provider.enabled ? 'is-paused' : ''}`} key={provider.id}><div className="provider-card-top"><ProviderMark {...provider} /><button type="button" role="switch" aria-checked={provider.enabled} aria-label={t('toggleNamed', { name: provider.name })} onClick={() => run(() => api(`/api/v1/providers/${provider.id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled: !provider.enabled }) })).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause)))} className={`toggle ${provider.enabled ? 'enabled' : ''}`}><span /></button></div><h3>{provider.name}</h3><div className="channel-card-meta"><span className="channel-tag"><Globe2 size={11} />{groupLabel(provider.group)}</span><span className="protocol-label">{protocolLabel(provider.type)}</span></div><p>{provider.models} {t('models')} · {provider.requests} {t('requestsTodayShort')}</p><div className="provider-card-stats"><span><small>{t('successRate')}</small><strong>{provider.success}</strong></span><span><small>{t('avgLatency')}</small><strong>{provider.latency ? `${provider.latency} ms` : '—'}</strong></span></div><div className="provider-card-foot"><span><StatusDot />{provider.enabled ? t('operational') : t('paused')}</span><button className="button text-button" onClick={() => { setEditing(provider); setDialogOpen(true) }}><Pencil size={12} />{t('manage')}<ArrowUpRight size={13} /></button></div></div>)}<button className="provider-card add-provider-card" onClick={() => { setEditing(null); setDialogOpen(true) }}><span><Plus size={22} /></span><h3>{t('bringProvider')}</h3><p>{t('bringProviderSubtitle')}</p></button></div> : providers.length === 0 ? <div className="empty-state glass-panel"><strong>{t('noProvidersConfigured')}</strong><p>{t('noProvidersConfiguredHint')}</p><button className="button primary" onClick={() => { setEditing(null); setDialogOpen(true) }}><Plus size={15} />{t('addProvider')}</button></div> : <div className="empty-state glass-panel">{t('noProviderMatch')}<button className="button text-button" onClick={() => { setQuery(''); setFilter('All'); setGroupFilter('all') }}>{t('clearFilters')}</button></div>}
    <ProviderDialog key={editing?.id || 'new'} open={dialogOpen} onClose={() => { setDialogOpen(false); setEditing(null) }} provider={editing} onSave={save} onTest={async (provider, secret) => (await run(() => api<{ models: string[] }>(`/api/v1/providers/${provider.id}/test`, { method: 'POST', body: JSON.stringify({ protocol: provider.type, endpoint: provider.endpoint, secret }) }))).models} onDelete={editing ? () => { setDeleting(editing); setDialogOpen(false) } : undefined} />
    <Modal open={!!deleting} onClose={() => setDeleting(null)} title={t('disconnectNamed', { name: deleting?.name || '' })} description={t('disconnectWarning')}><div className="delete-warning"><Trash2 size={17} />{routes.filter((r) => r.primary === deleting?.name || r.fallback === deleting?.name || r.candidates?.some((candidate) => candidate.provider === deleting?.name)).length} {t('routesReference')}</div><form onSubmit={(e) => { e.preventDefault(); remove() }}><DialogActions onClose={() => setDeleting(null)} label={t('confirmDisconnect')} /></form></Modal>
  </>
}
