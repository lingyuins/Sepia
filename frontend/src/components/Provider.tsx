import { useEffect, useState } from 'react'
import { Check, Copy, Play, Plus, RefreshCw, Settings2, X } from 'lucide-react'
import { DialogActions, Field, Modal, Toggle } from './UI'
import { Dropdown } from './Dropdown'
import { api } from '../api'
import { protocolOf, useTranslation, useWorkspace, type ProtocolType, type Provider } from '../workspace'
export type { Provider } from '../workspace'

export type ProviderKeyDraft = { value: string; models: string[] | null }
type KeyDraft = { id: number; value: string; configured: boolean; models: string[] | null; detected: string[] | null; error: string | null }
let keyDraftId = 0
const createKeyDraft = (configured = false, models: string[] | null = null): KeyDraft => ({ id: ++keyDraftId, value: '', configured, models, detected: null, error: null })
const keyDraftsFor = (provider?: Provider | null) => {
  const scopes = provider?.keyScopes
  if (scopes?.length) return scopes.map((scope) => createKeyDraft(true, scope.models))
  return Array.from({ length: Math.max(provider?.keyCount || 1, 1) }, () => createKeyDraft(Boolean(provider)))
}

export function ProviderMark({ name, short, color, small = false }: Pick<Provider, 'name' | 'short' | 'color'> & { small?: boolean }) {
  return <span className={`provider-mark ${small ? 'small' : ''}`} style={{ background: `${color}14`, color }} aria-label={name}>{short}</span>
}

export function ProviderDialog({ open, onClose, onSave, onDelete, onTest, provider }: { open: boolean; onClose: () => void; onSave: (provider: Provider, keys: ProviderKeyDraft[]) => Promise<void>; onDelete?: () => void; onTest?: (provider: Provider, keyIndex?: number, secret?: string) => Promise<string[]>; provider?: Provider | null }) {
  const [name, setName] = useState('')
  const { channelGroups, run, notify, settings } = useWorkspace()
  const t = useTranslation()
  const [type, setType] = useState<ProtocolType>('openai')
  const [group, setGroup] = useState('official')
  const [endpoint, setEndpoint] = useState(provider?.endpoint || '')
  const [enabled, setEnabled] = useState(provider?.enabled ?? true)
  const [testing, setTesting] = useState(false)
  const [tested, setTested] = useState(false)
  const [keyDrafts, setKeyDrafts] = useState<KeyDraft[]>(() => keyDraftsFor(provider))
  const [detectingKey, setDetectingKey] = useState<number | null>(null)
  const [groupOpen, setGroupOpen] = useState(false)
  const [groupName, setGroupName] = useState('')
  const [groupDescription, setGroupDescription] = useState('')
  const [models, setModels] = useState<string[]>(provider?.modelsList || [])
  const [modelDraft, setModelDraft] = useState('')
  const [modelsFetchedAt, setModelsFetchedAt] = useState(provider?.modelsFetchedAt || '')
  const [modelTested, setModelTested] = useState(false)
  const [fetchingModels, setFetchingModels] = useState(false)
  const [modelSettingsOpen, setModelSettingsOpen] = useState(false)
  const [modelCatalog, setModelCatalog] = useState<string[]>(provider?.modelCatalog || [])
  useEffect(() => {
    if (!open) return
    const nextType = provider ? protocolOf(provider) : 'openai'
    setName(provider?.name || '')
    setType(nextType)
    setGroup(provider?.group || 'official')
    setEndpoint(provider?.endpoint || '')
    setEnabled(provider?.enabled ?? true)
    setKeyDrafts(keyDraftsFor(provider))
    setTested(false)
    setModels([...(provider?.modelsList || [])])
    setModelDraft('')
    setModelsFetchedAt(provider?.modelsFetchedAt || '')
    setModelCatalog([...(provider?.modelCatalog || [])])
    setModelTested(false)
    setFetchingModels(false)
    setModelSettingsOpen(false)
    setGroupOpen(false)
    setDetectingKey(null)
  }, [open, provider])
  const modelListLabel = modelsFetchedAt
    ? Date.now() - new Date(modelsFetchedAt).getTime() < 60000
      ? t('justNow')
      : new Intl.DateTimeFormat(settings.language === '简体中文' ? 'zh-CN' : settings.language === '日本語' ? 'ja-JP' : 'en-US', { month: 'short', day: 'numeric' }).format(new Date(modelsFetchedAt))
    : t('notFetched')
  const changeType = (value: string) => {
    const nextType = value as ProtocolType
    setType(nextType)
    setModelsFetchedAt('')
    setTested(false)
    setModelTested(false)
  }
  const addModel = () => {
    const id = modelDraft.trim()
    if (!id) { notify(t('modelIdRequired')); return }
    if (models.includes(id)) { notify(t('modelExists')); return }
    setModels((current) => [...current, id])
    setModelCatalog((current) => [...current, id])
    setModelDraft('')
    setModelTested(false)
    notify(t('modelAdded', { name: id }))
  }
  const discoverKey = (index: number) => {
    const key = keyDrafts[index]
    const secret = key.value.trim()
    if (provider && onTest) return onTest({ ...provider, type, endpoint }, index, secret || undefined)
    return run(() => api<{ models: string[] }>('/api/v1/providers/test', { method: 'POST', body: JSON.stringify({ protocol: type, endpoint, secret: secret || '', secrets: keyDrafts.map((item) => item.value.trim()).filter(Boolean) }) })).then((result) => result.models)
  }
  const detectKeyModels = async (id: number) => {
    const index = keyDrafts.findIndex((key) => key.id === id)
    if (index < 0) return
    setDetectingKey(id)
    try {
      const discovered = await discoverKey(index)
      setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, models: [...discovered], detected: [...discovered], error: null } : key))
      setModels((current) => [...new Set([...current, ...discovered])])
      setModelCatalog((current) => [...new Set([...current, ...discovered])])
      setModelsFetchedAt(new Date().toISOString())
      setModelTested(false)
      notify(t('keyModelsDetected', { number: index + 1, count: discovered.length }))
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause)
      setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, error: message } : key))
      notify(message)
    } finally { setDetectingKey(null) }
  }
  const detectAllKeys = async () => {
    const results = await Promise.all(keyDrafts.map(async (key, index) => {
      try { return { id: key.id, models: await discoverKey(index), error: null as string | null } } catch (cause) { return { id: key.id, models: null, error: cause instanceof Error ? cause.message : String(cause) } }
    }))
    setKeyDrafts((current) => current.map((key) => {
      const result = results.find((item) => item.id === key.id)
      if (!result) return key
      return result.models ? { ...key, models: [...result.models], detected: [...result.models], error: null } : { ...key, error: result.error }
    }))
    const discovered = [...new Set(results.flatMap((result) => result.models || []))]
    if (discovered.length) {
      setModels(discovered)
      setModelCatalog(discovered)
      setModelsFetchedAt(new Date().toISOString())
      setModelTested(false)
    }
    return results
  }
  const refreshModels = async () => {
    if (!provider && (!name.trim() || !endpoint.trim() || !keyDrafts.some((key) => key.value.trim()))) { notify(t('modelsRequired')); return }
    setFetchingModels(true)
    try { const results = await detectAllKeys(); const count = results.flatMap((result) => result.models || []).length; if (count) notify(t('modelsRefreshed', { count })); const failed = results.find((result) => result.error); if (failed?.error) notify(failed.error) } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setFetchingModels(false) }
  }
  const testModels = async () => {
    setFetchingModels(true)
    try { const results = await detectAllKeys(); const discovered = results.flatMap((result) => result.models || []); setModelTested(discovered.length > 0 && models.every((model) => discovered.includes(model))); notify(t('modelsTested', { count: models.filter((model) => discovered.includes(model)).length })); const failed = results.find((result) => result.error); if (failed?.error) notify(failed.error) } catch (cause) { setModelTested(false); notify(cause instanceof Error ? cause.message : String(cause)) } finally { setFetchingModels(false) }
  }
  const clearModels = () => {
    setModels([])
    setModelTested(false)
    notify(t('modelsCleared'))
  }
  const toggleModel = (model: string) => {
    setModels((current) => current.includes(model) ? current.filter((item) => item !== model) : [...current, model])
    setModelTested(false)
  }
  const addKey = () => setKeyDrafts((current) => [...current, createKeyDraft()])
  const updateKey = (id: number, value: string) => setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, value, models: null, detected: null, error: null } : key))
  const removeKey = (id: number) => setKeyDrafts((current) => current.length > 1 ? current.filter((key) => key.id !== id) : current)
  const toggleKeyModel = (id: number, model: string) => setKeyDrafts((current) => current.map((key) => {
    if (key.id !== id) return key
    const scope = key.models || []
    return { ...key, models: scope.includes(model) ? scope.filter((item) => item !== model) : [...scope, model] }
  }))
  const clearKeyScope = (id: number) => setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, models: null } : key))
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!name.trim()) { notify(t('channelNameRequired')); return }
    if (!provider && !keyDrafts.some((key) => key.value.trim())) { notify(t('apiKeyRequired')); return }
    if (!models.length) { notify(t('modelsRequired')); return }
    try {
      const keyCount = keyDrafts.filter((key) => key.configured || key.value.trim()).length
      const saved = { id: provider?.id || Date.now(), name: name.trim(), short: name.trim().charAt(0).toUpperCase(), type, group, models: models.length, latency: provider?.latency || 0, success: provider?.success || '—', requests: provider?.requests || '0', color: provider?.color || '#c4f4aa', enabled, endpoint, modelsList: models, keyCount, keyScopes: provider?.keyScopes }
      await onSave(saved, keyDrafts.filter((key) => key.configured || key.value.trim()).map((key) => ({ value: key.value.trim(), models: key.models })))
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const createGroup = async (event: React.FormEvent) => {
    event.preventDefault()
    const name = groupName.trim()
    if (!name) { notify(t('groupNameRequired')); return }
    if (channelGroups.some((item) => item.name.toLowerCase() === name.toLowerCase())) { notify(t('groupExists')); return }
    const id = `group-${Date.now()}`
    try { await run(() => api('/api/v1/groups', { method: 'POST', body: JSON.stringify({ id, name, description: groupDescription.trim() }) })); setGroup(id); setGroupOpen(false); setGroupName(''); setGroupDescription(''); notify(t('groupCreated')) } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const groupLabel = (id: string) => {
    if (id === 'official') return t('officialChannels')
    if (id === 'free') return t('freeChannels')
    if (id === 'team') return t('teamChannels')
    return channelGroups.find((item) => item.id === id)?.name || t('noGroup')
  }
  const protocolOptions = [
    { value: 'openai', label: t('openaiCompatible'), description: t('openaiProtocolHint'), badge: 'OpenAI' },
    { value: 'anthropic', label: t('anthropicCompatible'), description: t('anthropicProtocolHint'), badge: 'Claude' },
    { value: 'gemini', label: t('geminiCompatible'), description: t('geminiProtocolHint'), badge: 'Gemini' },
    { value: 'openai-responses', label: t('openaiResponsesCompatible'), description: t('openaiResponsesProtocolHint'), badge: 'Responses' },
  ]
  return <>{!groupOpen && !modelSettingsOpen && <Modal open={open} onClose={onClose} title={provider ? t('manageProvider', { name: provider.name }) : t('connectProviderTitle')} description={t('providerDialogSubtitle')} size="channel-dialog">
    <form onSubmit={submit}>
      <div className="channel-section-title"><span className="channel-section-icon"><Check size={14} /></span><h3>{t('basicInformation')}</h3></div>
      <div className="channel-fields">
        <Field label={t('channelName')} value={name} onChange={setName} required placeholder={t('channelNamePlaceholder')} />
        <Dropdown label={t('providerType')} value={type} options={protocolOptions} onChange={changeType} />
        <Dropdown label={t('channelGroup')} value={group} options={channelGroups.map((item) => ({ value: item.id, label: groupLabel(item.id), description: item.id === 'official' ? t('officialGroupHint') : item.id === 'free' ? t('freeGroupHint') : item.id === 'team' ? t('teamGroupHint') : item.description }))} onChange={setGroup} onCreate={() => setGroupOpen(true)} />
      </div>
      <div className="channel-connection-fields">
        <div className="key-manager">
          <div className="key-manager-heading"><span className="field-label">{t('apiKeys')}</span><button type="button" className="button text-button" onClick={addKey}><Plus size={12} />{t('addKey')}</button></div>
          <div className="key-input-list">{keyDrafts.map((key, index) => <div className="key-input-item" key={key.id}>
            <div className="key-input-row"><input className="field" type="password" value={key.value} onChange={(event) => updateKey(key.id, event.target.value)} placeholder={key.configured ? t('notStored') : t('apiKeyPlaceholder')} aria-label={t('apiKeyNumber', { number: index + 1 })} autoComplete="new-password" /><button type="button" className="button secondary key-detect-button" disabled={detectingKey !== null || fetchingModels || !endpoint.trim() || (!provider && !key.value.trim())} onClick={() => detectKeyModels(key.id)}><RefreshCw className={detectingKey === key.id ? 'spin' : ''} size={12} />{t('detectKeyModels')}</button><button type="button" className="icon-button" disabled={keyDrafts.length === 1} onClick={() => removeKey(key.id)} aria-label={t('removeApiKey', { number: index + 1 })}><X size={14} /></button></div>
            {key.models === null
              ? <small className="field-hint">{key.configured && !key.value.trim() ? t('keyScopeStored') : t('keyScopeAll')}</small>
              : <div className="key-scope-chips">{models.map((model) => <label className={`key-scope-chip ${key.models?.includes(model) ? 'selected' : ''}`} key={model}><input type="checkbox" checked={key.models?.includes(model) || false} onChange={() => toggleKeyModel(key.id, model)} /><code>{model}</code></label>)}<button type="button" className="button text-button" onClick={() => clearKeyScope(key.id)}>{t('keyScopeAll')}</button></div>}
            {key.error && <small className="key-scope-error">{key.error}</small>}
          </div>)}</div>
          <small className="field-hint">{t('multipleKeysHint')} {t('keyScopeHint')}</small>
        </div>
        <Field label={t('baseUrl')} type="url" value={endpoint} onChange={setEndpoint} placeholder="https://api.example.com/v1" />
      </div>
      <section className="model-manager">
        <div className="channel-section-title"><span className="channel-section-icon"><Settings2 size={14} /></span><h3>{t('modelManagement')}</h3><span className="model-count">{models.length} {t('models')}</span></div>
        <div className="model-toolbar">
          <input className="field model-id-input" value={modelDraft} onChange={(event) => setModelDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addModel() } }} placeholder={t('modelIdPlaceholder')} aria-label={t('modelIdPlaceholder')} />
          <button type="button" className="button secondary" disabled={!modelDraft.trim() || fetchingModels} onClick={addModel}><Plus size={13} />{t('addModel')}</button>
          <button type="button" className="button primary" disabled={fetchingModels || !endpoint.trim() || (!provider && (!name.trim() || !keyDrafts.some((key) => key.value.trim())))} onClick={refreshModels}><RefreshCw className={fetchingModels ? 'spin' : ''} size={13} />{fetchingModels ? t('fetchingModels') : t('fetchModels')}</button>
          <button type="button" className="button secondary" disabled={!models.length || fetchingModels} onClick={testModels}><Play size={13} />{t('testModels')}</button>
          <button type="button" className="button secondary" onClick={() => setModelSettingsOpen(true)}><Settings2 size={13} />{t('modelSettings')}</button>
          <span className="model-list-meta">{t('providerList')} · {modelListLabel}</span>
          <button type="button" className="button secondary" disabled={!models.length} onClick={clearModels}>{t('clear')}</button>
        </div>
        <div className={`model-chip-list ${models.length ? '' : 'is-empty'}`}>
          {models.map((model) => <span className="model-chip" key={model}><code>{model}</code><button type="button" onClick={() => { setModels((current) => current.filter((item) => item !== model)); setModelTested(false) }} aria-label={t('removeModel', { name: model })}><X size={12} /></button></span>)}
          {!models.length && <span>{t('noModelsConfigured')}</span>}
        </div>
        {modelTested && <div className="model-test-result"><Check size={13} />{t('modelsTested', { count: models.length })}</div>}
      </section>
      <div className="switch-row"><span>{t('enableProvider')}</span><Toggle label={t('enableProvider')} checked={enabled} onChange={() => setEnabled(!enabled)} /></div>
      <div className="provider-test"><button type="button" className="button secondary" disabled={testing || fetchingModels || !endpoint.trim() || (!provider && (!name.trim() || !keyDrafts.some((key) => key.value.trim())))} onClick={async () => { setTesting(true); try { const results = await detectAllKeys(); setTested(results.some((result) => result.models?.length)); const failed = results.find((result) => result.error); if (failed?.error) notify(failed.error) } catch (cause) { setTested(false); notify(cause instanceof Error ? cause.message : String(cause)) } finally { setTesting(false) } }}>{testing || fetchingModels ? t('testingConnection') : t('testConnection')}</button>{tested && <span><Check size={13} />{t('configLooksGood')}</span>}</div>
      <DialogActions onClose={onClose} label={provider ? t('saveProvider') : t('connectProvider')} />
      {provider && onDelete && <button type="button" className="dialog-danger-link" onClick={onDelete}>{t('disconnectProvider')}</button>}
    </form>
  </Modal>}
  <Modal open={modelSettingsOpen} onClose={() => setModelSettingsOpen(false)} title={t('modelSettings')} description={t('modelSettingsSubtitle')} size="nested-dialog model-settings-dialog">
    <div className="model-settings-body">
      <div className="model-catalog-heading"><span>{t('catalogModels')}</span><small>{models.length} {t('models')}</small></div>
      <div className="model-selection-list">{modelCatalog.map((model) => <label className="model-setting-option" key={model}><input type="checkbox" checked={models.includes(model)} onChange={() => toggleModel(model)} /><code>{model}</code></label>)}</div>
    </div>
    <div className="dialog-actions"><button type="button" className="button primary" onClick={() => setModelSettingsOpen(false)}>{t('done')}</button></div>
  </Modal>
  <Modal open={groupOpen} onClose={() => setGroupOpen(false)} title={t('createGroup')} description={t('groupDescriptionPlaceholder')} size="nested-dialog">
    <form onSubmit={createGroup}><Field label={t('groupName')} value={groupName} onChange={setGroupName} required placeholder={t('groupNamePlaceholder')} /><Field label={t('groupDescription')} value={groupDescription} onChange={setGroupDescription} placeholder={t('groupDescriptionPlaceholder')} /><DialogActions onClose={() => setGroupOpen(false)} label={t('createGroup')} /></form>
  </Modal>
  </>
}

export function Toast({ message, onClose }: { message: string; onClose: () => void }) {
  return <div className="toast" role="status"><span className="toast-check"><Check size={14} /></span>{message}<ToastClose onClose={onClose} /></div>
}
function ToastClose({ onClose }: { onClose: () => void }) {
  return <button onClick={onClose} aria-label={useTranslation()('dismiss')}><X size={14} /></button>
}
export function CopyButton({ value, onCopy }: { value: string; onCopy: () => void }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try { await navigator.clipboard.writeText(value) } catch { const input = document.createElement('textarea'); input.value = value; document.body.appendChild(input); input.select(); document.execCommand('copy'); input.remove() }
    setCopied(true); onCopy(); setTimeout(() => setCopied(false), 1800)
  }
  return <button className="icon-button" onClick={copy} aria-label={useTranslation()('copyClipboard')}>{copied ? <Check size={15} /> : <Copy size={15} />}</button>
}
