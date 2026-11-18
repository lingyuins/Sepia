import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Blocks, Download, PackagePlus, Search, Settings2, Trash2, Upload, X } from 'lucide-react'
import { useWorkspace, useTranslation, type Plugin } from '../workspace'
import { api } from '../api'
import { DialogActions, Field, Modal, SectionHeading, Toggle } from '../components/UI'

type ConfigSchema = { type?: string; properties?: Record<string, { type?: string; description?: string; default?: unknown }> }

function ConfigEditor({ plugin, onClose }: { plugin: Plugin; onClose: () => void }) {
  const { notify, run } = useWorkspace()
  const t = useTranslation()
  const schema = plugin.configSchema as ConfigSchema
  const properties = schema.properties || {}
  const [draft, setDraft] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {}
    for (const key of Object.keys(properties)) {
      const value = plugin.config[key]
      initial[key] = value === undefined || value === null ? '' : typeof value === 'object' ? JSON.stringify(value) : String(value)
    }
    return initial
  })
  const [saving, setSaving] = useState(false)
  const save = async () => {
    const config: Record<string, unknown> = {}
    for (const [key, property] of Object.entries(properties)) {
      const raw = (draft[key] || '').trim()
      if (!raw) continue
      if (property.type === 'number' || property.type === 'integer') {
        const parsed = Number(raw)
        if (!Number.isFinite(parsed)) { notify(t('pluginConfigInvalid')); return }
        config[key] = property.type === 'integer' ? Math.round(parsed) : parsed
      } else if (property.type === 'boolean') {
        config[key] = raw === 'true'
      } else if (property.type === 'array' || property.type === 'object') {
        try { config[key] = JSON.parse(raw) } catch { notify(t('pluginConfigInvalid')); return }
      } else {
        config[key] = raw
      }
    }
    setSaving(true)
    try {
      await run(() => api(`/api/v1/plugins/${plugin.id}/config`, { method: 'PUT', body: JSON.stringify(config) }))
      notify(t('pluginConfigSaved'))
      onClose()
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setSaving(false) }
  }
  const keys = Object.keys(properties)
  return <form onSubmit={(event) => { event.preventDefault(); void save() }}>
    {keys.length === 0 && <p className="plugin-config-empty">{t('pluginNoConfig')}</p>}
    {keys.map((key) => <Field key={key} label={key} value={draft[key] || ''} onChange={(value) => setDraft((current) => ({ ...current, [key]: value }))} placeholder={properties[key].default === undefined ? '' : String(properties[key].default)} hint={properties[key].description} />)}
    <DialogActions onClose={onClose} label={t('saveChanges')} disabled={saving} />
  </form>
}

function PluginUi({ plugin, onClose }: { plugin: Plugin; onClose: () => void }) {
  const { notify } = useWorkspace()
  const t = useTranslation()
  const frame = useRef<HTMLIFrameElement>(null)
  const [document_, setDocument] = useState('')
  const session = useRef('')
  useEffect(() => {
    let cancelled = false
    api<{ session: string; document: string }>(`/api/v1/plugins/${plugin.id}/ui-bundle`)
      .then((response) => { if (!cancelled) { session.current = response.session; setDocument(response.document) } })
      .catch((cause) => { if (!cancelled) notify(cause instanceof Error ? cause.message : String(cause)) })
    return () => { cancelled = true }
  }, [plugin.id, notify])
  useEffect(() => {
    const handler = async (event: MessageEvent) => {
      const source = event.source
      if (!frame.current || source !== frame.current.contentWindow) return
      if (event.origin !== 'null') return
      const data = event.data as { session?: string; id?: string; method?: string; params?: unknown }
      if (!data || data.session !== session.current || typeof data.id !== 'string' || typeof data.method !== 'string') return
      const reply = (result: unknown, error?: string) => {
        frame.current?.contentWindow?.postMessage({ session: session.current, id: data.id, result, error }, '*')
      }
      try {
        const result = await api(`/api/v1/plugins/${plugin.id}/rpc/${data.method.replace(/^\/+/, '')}`, { method: 'POST', body: JSON.stringify(data.params ?? {}) })
        reply(result)
      } catch (cause) {
        reply(null, cause instanceof Error ? cause.message : String(cause))
      }
    }
    window.addEventListener('message', handler)
    return () => window.removeEventListener('message', handler)
  }, [plugin.id])
  return <div className="plugin-ui-frame">
    {document_
      ? <iframe ref={frame} title={plugin.uiTitle || plugin.name} sandbox="allow-scripts" srcDoc={document_} className="plugin-iframe" />
      : <p className="plugin-config-empty">{t('pluginUiLoading')}</p>}
    <DialogActions onClose={onClose} label={t('closeDialog')} />
  </div>
}

export function PluginsView() {
  const { plugins, notify, run } = useWorkspace()
  const t = useTranslation()
  const [query, setQuery] = useState('')
  const [installOpen, setInstallOpen] = useState(false)
  const [packageFile, setPackageFile] = useState<File | null>(null)
  const [installing, setInstalling] = useState(false)
  const [configTarget, setConfigTarget] = useState<Plugin | null>(null)
  const [uiTarget, setUiTarget] = useState<Plugin | null>(null)
  const [uninstallTarget, setUninstallTarget] = useState<Plugin | null>(null)
  const [busyId, setBusyId] = useState('')
  const fileInput = useRef<HTMLInputElement>(null)
  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return plugins.filter((plugin) => !needle || plugin.name.toLowerCase().includes(needle) || plugin.id.toLowerCase().includes(needle) || plugin.description.toLowerCase().includes(needle))
  }, [plugins, query])
  const install = useCallback(async () => {
    if (!packageFile) { notify(t('pluginChoosePackage')); return }
    setInstalling(true)
    try {
      const form = new FormData()
      form.append('package', packageFile)
      await run(() => api('/api/v1/plugins/install', { method: 'POST', body: form }))
      notify(t('pluginInstalled'))
      setInstallOpen(false)
      setPackageFile(null)
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setInstalling(false) }
  }, [packageFile, notify, run, t])
  const toggle = useCallback(async (plugin: Plugin) => {
    setBusyId(plugin.id)
    try {
      await run(() => api(`/api/v1/plugins/${plugin.id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled: !plugin.enabled }) }))
      notify(plugin.enabled ? t('pluginDisabled') : t('pluginEnabled'))
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setBusyId('') }
  }, [notify, run, t])
  const uninstall = useCallback(async () => {
    if (!uninstallTarget) return
    setBusyId(uninstallTarget.id)
    try {
      await run(() => api(`/api/v1/plugins/${uninstallTarget.id}`, { method: 'DELETE' }))
      notify(t('pluginUninstalled'))
      setUninstallTarget(null)
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setBusyId('') }
  }, [uninstallTarget, notify, run, t])
  return <>
    <SectionHeading eyebrow={t('pluginEyebrow')} title={t('pluginsTitle')} description={t('pluginsSubtitle')} action={<button className="button" onClick={() => setInstallOpen(true)}><PackagePlus size={16} />{t('installPlugin')}</button>} />
    <div className="plugin-toolbar glass-panel">
      <label className="search-field"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t('findPlugin')} aria-label={t('findPlugin')} /></label>
      <span className="plugin-count">{t('pluginCount').replace('{count}', String(plugins.length))}</span>
    </div>
    {visible.length === 0
      ? <div className="plugin-banner glass-panel"><div className="plugin-banner-icon"><Blocks size={26} /></div><div><h3>{plugins.length === 0 ? t('pluginEmptyTitle') : t('nothingHere')}</h3><p>{plugins.length === 0 ? t('pluginEmptySubtitle') : t('pluginNoMatch')}</p></div></div>
      : <div className="plugin-grid">
        {visible.map((plugin) => <article key={plugin.id} className={`plugin-card glass-panel ${plugin.enabled ? '' : 'plugin-card-disabled'}`}>
          <header className="plugin-card-head">
            <div className="plugin-card-icon"><Blocks size={20} /></div>
            <div className="plugin-card-title"><h3>{plugin.name}</h3><span className="plugin-version">v{plugin.version}</span></div>
            <span className={`plugin-status ${plugin.enabled ? 'plugin-status-on' : ''}`}>{plugin.enabled ? t('enabled') : t('disabled')}</span>
          </header>
          <p className="plugin-card-desc">{plugin.description || t('pluginNoDescription')}</p>
          <div className="plugin-chips">
            {plugin.permissions.map((permission) => <span key={permission} className="plugin-chip">{permission}</span>)}
          </div>
          <footer className="plugin-card-actions">
            <Toggle checked={plugin.enabled} onChange={() => void toggle(plugin)} label={plugin.enabled ? t('disablePlugin') : t('enablePlugin')} />
            <div className="plugin-card-buttons">
              <button className="button text-button" title={t('pluginConfigure')} onClick={() => setConfigTarget(plugin)}><Settings2 size={15} /><span>{t('pluginConfigure')}</span></button>
              {plugin.hasUi && <button className="button text-button" title={t('pluginOpenUi')} onClick={() => setUiTarget(plugin)}><Upload size={15} /><span>{t('pluginOpenUi')}</span></button>}
              <button className="button text-button plugin-danger" title={t('uninstall')} disabled={busyId === plugin.id} onClick={() => setUninstallTarget(plugin)}><Trash2 size={15} /><span>{t('uninstall')}</span></button>
            </div>
          </footer>
        </article>)}
      </div>}
    <Modal open={installOpen} onClose={() => setInstallOpen(false)} title={t('installPlugin')} description={t('installDescription')}>
      <form onSubmit={(event) => { event.preventDefault(); void install() }}>
        <input ref={fileInput} type="file" accept=".zip" className="plugin-file-input" onChange={(event) => setPackageFile(event.target.files?.[0] || null)} />
        <button type="button" className="button secondary plugin-choose" onClick={() => fileInput.current?.click()}><Download size={15} />{packageFile ? packageFile.name : t('choosePackage')}</button>
        <p className="plugin-package-hint">{t('pluginPackageHint')}</p>
        <DialogActions onClose={() => setInstallOpen(false)} label={installing ? t('installing') : t('installLocally')} disabled={installing || !packageFile} />
      </form>
    </Modal>
    <Modal open={configTarget !== null} onClose={() => setConfigTarget(null)} title={configTarget ? `${t('pluginConfigure')} · ${configTarget.name}` : ''} description={t('pluginConfigDescription')}>
      {configTarget && <ConfigEditor plugin={configTarget} onClose={() => setConfigTarget(null)} />}
    </Modal>
    <Modal open={uiTarget !== null} onClose={() => setUiTarget(null)} title={uiTarget ? (uiTarget.uiTitle || uiTarget.name) : ''} size="route-dialog">
      {uiTarget && <PluginUi plugin={uiTarget} onClose={() => setUiTarget(null)} />}
    </Modal>
    <Modal open={uninstallTarget !== null} onClose={() => setUninstallTarget(null)} title={t('uninstallPlugin')} description={uninstallTarget ? `${uninstallTarget.name} · ${t('uninstallDescription')}` : ''}>
      <form onSubmit={(event) => { event.preventDefault(); void uninstall() }}>
        <DialogActions onClose={() => setUninstallTarget(null)} label={t('uninstall')} disabled={busyId !== ''} leading={<X size={15} />} />
      </form>
    </Modal>
  </>
}