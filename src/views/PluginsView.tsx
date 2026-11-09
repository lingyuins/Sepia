import { useState } from 'react'
import { Activity, ArrowUpRight, Blocks, Download, FileCode2, Globe2, Plus, Route, Search, Settings, ShieldCheck, TerminalSquare, Trash2, Zap } from 'lucide-react'
import { downloadFile, useWorkspace, useTranslation, type Plugin } from '../workspace'
import { DialogActions, Modal, Toggle } from '../components/UI'
import { SectionHeading } from './ProvidersView'

const icons = { route: Route, activity: Activity, zap: Zap, shield: ShieldCheck, local: Blocks }
export function PluginsView() {
  const { plugins, setPlugins, notify, settings } = useWorkspace()
  const t = useTranslation()
  const typeName = (type: string) => type === 'Routing' ? t('routingType') : type === 'Middleware' ? t('middleware') : type === 'Observability' ? t('observabilityType') : type === 'Local' ? t('local') : t('all')
  const pluginDescription = (plugin: Plugin) => plugin.id === 1 ? t('smartRouterDescription') : plugin.id === 2 ? t('inspectorDescription') : plugin.id === 3 ? t('cacheDescription') : plugin.id === 4 ? t('privacyDescription') : t('localPluginDescription')
  const permissionName = (permission: string) => permission === 'Network access' ? t('networkAccess') : permission === 'No default permissions' ? t('noPermissions') : permission === 'Read routing candidates' ? t('smartRouterPermission1') : permission === 'Read provider health' ? t('smartRouterPermission2') : permission === 'Read request metadata' ? t('inspectorPermission') : permission === 'Read requests' ? t('cachePermission1') : permission === 'Store cached responses' ? t('cachePermission2') : permission === 'Transform request metadata' ? t('privacyPermission') : permission
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('All')
  const [installOpen, setInstallOpen] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [selected, setSelected] = useState<Plugin | null>(null)
  const [deleting, setDeleting] = useState<Plugin | null>(null)
  const [packageName, setPackageName] = useState('')
  const filtered = plugins.filter((p) => `${p.name} ${p.type}`.toLowerCase().includes(query.toLowerCase()) && (filter === 'All' || p.type === filter))
  const installPlugin = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const file = new FormData(event.currentTarget).get('plugin') as File
    if (!file || file.size > 16 * 1024 * 1024) { notify(t('packageTooLarge')); return }
    setInstalling(true)
    setTimeout(() => {
      const plugin: Plugin = { id: Date.now(), name: file.name.replace(/\.(wasm|sepia|zip)$/i, ''), description: t('localPluginDescription'), version: '0.1.0', type: 'Local', icon: 'local', enabled: false, color: 'mint', author: t('localPublisher'), permissions: settings.pluginNetwork ? ['Network access'] : ['No default permissions'], installed: t('pluginDate') }
      setPlugins((current) => [...current, plugin]); setInstalling(false); setInstallOpen(false); setPackageName(''); notify(t('pluginInstalled'))
    }, 500)
  }
  const sdk = () => {
    downloadFile('sepia-plugin-example.json', JSON.stringify({ apiVersion: 'sepia.dev/v1', name: 'my-first-plugin', version: '0.1.0', description: 'A thoughtful extension for your gateway.', permissions: ['request.metadata.read'], entry: 'plugin.wasm', hooks: ['relay.candidate.filter', 'relay.completed'] }, null, 2))
    notify(t('manifestDownloaded'))
  }
  return <>
    <SectionHeading eyebrow={t('pluginEyebrow')} title={t('pluginsTitle')} description={t('pluginsSubtitle')} action={<button className="button primary" onClick={() => setInstallOpen(true)}><Plus size={16} />{t('installPlugin')}</button>} />
    <div className="plugin-banner glass-panel"><div className="plugin-banner-icon"><Blocks size={26} /></div><div><h3>{t('pluginBannerTitle')}</h3><p>{t('pluginBannerSubtitle')}</p></div><span className="version-tag">{t('pluginRuntime')}</span></div>
    <div className="list-toolbar"><div className="search-field"><Search size={15} /><input aria-label={t('findPlugin')} placeholder={t('findPlugin')} value={query} onChange={(e) => setQuery(e.target.value)} /></div><div className="filter-tabs">{['All', 'Routing', 'Middleware', 'Observability', 'Local'].map((item) => <button key={item} className={filter === item ? 'active' : ''} onClick={() => setFilter(item)}>{typeName(item)}</button>)}</div></div>
    {filtered.length ? <div className="plugins-grid">{filtered.map((plugin) => { const Icon = icons[plugin.icon as keyof typeof icons] || Blocks; return <div className={`plugin-card glass-panel ${!plugin.enabled ? 'is-paused' : ''}`} key={plugin.id}><div className="plugin-card-top"><button className={`plugin-icon ${plugin.color}`} onClick={() => setSelected(plugin)} aria-label={t('viewNamed', { name: plugin.name })}><Icon size={21} /></button><Toggle checked={plugin.enabled} onChange={() => { setPlugins((current) => current.map((p) => p.id === plugin.id ? { ...p, enabled: !p.enabled } : p)); notify(t('pluginStateChanged', { name: plugin.name, state: plugin.enabled ? t('disabled') : t('enabled') })) }} label={t('toggleNamed', { name: plugin.name })} /></div><div className="plugin-type">{typeName(plugin.type)}</div><h3>{plugin.name}</h3><p>{pluginDescription(plugin)}</p><div className="plugin-card-foot"><span>v{plugin.version}</span><span className={plugin.enabled ? 'positive' : 'muted'}>{plugin.enabled ? t('enabled') : t('disabled')}</span><button className="icon-button" onClick={() => setSelected(plugin)} aria-label={t('configureNamed', { name: plugin.name })}><Settings size={15} /></button></div></div>})}</div> : <div className="empty-state glass-panel">{t('nothingHere')}<button className="button text-button" onClick={() => { setFilter('All'); setQuery('') }}>{t('showAllPlugins')}</button></div>}
    <div className="developer-banner"><TerminalSquare size={19} /><div><strong>{t('haveSomething')}</strong><p>{t('manifestSubtitle')}</p></div><button className="button secondary" onClick={sdk}><Download size={14} />{t('downloadManifest')}</button></div>
    <Modal open={installOpen} onClose={() => setInstallOpen(false)} title={t('installYourOwn')} description={t('installDescription')}><form onSubmit={installPlugin}><label className="field-label" htmlFor="plugin-file">{t('pluginPackage')}</label><label className={`plugin-dropzone ${packageName ? 'has-file' : ''}`}><FileCode2 size={27} /><strong>{packageName || t('choosePackage')}</strong><span>{t('packageHint')}</span><input id="plugin-file" type="file" name="plugin" required accept=".wasm,.sepia,.zip" onChange={(e) => setPackageName(e.target.files?.[0]?.name || '')} /></label><div className="secure-note"><ShieldCheck size={15} />{t('packagePreviewNote')}</div><DialogActions onClose={() => setInstallOpen(false)} label={installing ? t('installing') : t('installLocally')} disabled={installing} /></form></Modal>
    <Modal open={!!selected} onClose={() => setSelected(null)} title={selected?.name || ''} description={selected ? pluginDescription(selected) : ''}><div className="plugin-details"><div className="plugin-meta"><span><small>{t('version')}</small><strong>v{selected?.version}</strong></span><span><small>{t('publisher')}</small><strong>{selected?.author}</strong></span><span><small>{t('installed')}</small><strong>{selected?.installed}</strong></span></div><h4>{t('permissions')}</h4>{selected?.permissions.map((permission) => <div className="permission-row" key={permission}><ShieldCheck size={14} /><span>{permissionName(permission)}</span><span className="permission-status">{t('declared')}</span></div>)}<div className="settings-inline-note"><Globe2 size={14} />{settings.pluginNetwork ? t('networkAllowed') : t('networkReviewed')}</div></div><div className="dialog-actions spread-actions"><button className="button text-button danger-text" onClick={() => { setDeleting(selected); setSelected(null) }}><Trash2 size={13} />{t('uninstall')}</button><button className="button primary" onClick={() => { if (selected) setPlugins((current) => current.map((p) => p.id === selected.id ? { ...p, enabled: !p.enabled } : p)); notify(t('pluginStateChanged', { name: selected?.name || '', state: selected?.enabled ? t('disabled') : t('enabled') })); setSelected(null) }}>{selected?.enabled ? t('disablePlugin') : t('enablePlugin')}<ArrowUpRight size={14} /></button></div></Modal>
    <Modal open={!!deleting} onClose={() => setDeleting(null)} title={t('uninstallNamed', { name: deleting?.name || '' })} description={t('uninstallDescription')}><form onSubmit={(e) => { e.preventDefault(); setPlugins((current) => current.filter((p) => p.id !== deleting?.id)); setDeleting(null); notify(t('pluginUninstalled')) }}><DialogActions onClose={() => setDeleting(null)} label={t('uninstallPlugin')} /></form></Modal>
  </>
}
