import { useEffect, useState } from 'react'
import { Check, Copy, Play, Plus, RefreshCw, Settings2, X } from 'lucide-react'
import { DialogActions, Field, Modal, Toggle } from './UI'
import { Dropdown } from './Dropdown'
import { api } from '../api'
import { protocolOf, useTranslation, useWorkspace, type ProtocolType, type Provider } from '../workspace'
export type { Provider } from '../workspace'

export type ProviderKeyDraft = { value: string; models: string[] | null }
type KeyProbe = { ok: boolean; model: string; latency_ms: number; endpoint: string | null; error: string | null; reply: string | null }
type KeyDraft = { id: number; value: string; configured: boolean; models: string[] | null; detected: string[] | null; error: string | null; probe: KeyProbe | null }
let keyDraftId = 0
const createKeyDraft = (configured = false, models: string[] | null = null): KeyDraft => ({ id: ++keyDraftId, value: '', configured, models, detected: null, error: null, probe: null })
const keyDraftsFor = (provider?: Provider | null) => {
  const scopes = provider?.keyScopes
  if (scopes?.length) return scopes.map((scope) => createKeyDraft(true, scope.models))
  return Array.from({ length: Math.max(provider?.keyCount || 1, 1) }, () => createKeyDraft(Boolean(provider)))
}
const maskKey = (value: string) => value.length > 12 ? `${value.slice(0, 4)}…${value.slice(-4)}` : '••••'

export function ProviderMark({ name, short, color, small = false }: Pick<Provider, 'name' | 'short' | 'color'> & { small?: boolean }) {
  return <span className={`provider-mark ${small ? 'small' : ''}`} style={{ background: `${color}14`, color }} aria-label={name}>{short}</span>
}

export function ProviderDialog({ open, onClose, onSave, onDelete, onTest, provider }: { open: boolean; onClose: () => void; onSave: (provider: Provider, keys: ProviderKeyDraft[]) => Promise<void>; onDelete?: () => void; onTest?: (provider: Provider, keyIndex?: number, secret?: string) => Promise<string[]>; provider?: Provider | null }) {
  const [name, setName] = useState('')
  const { channelGroups, run, notify } = useWorkspace()
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
  const [modelSettingsOpen, setModelSettingsOpen] = useState(false)
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
    setModelSettingsOpen(false)
    setGroupOpen(false)
    setDetectingKey(null)
    setTesting(false)
  }, [open, provider])
  const changeType = (value: string) => {
    const nextType = value as ProtocolType
    setType(nextType)
    setTested(false)
  }
  const addModel = () => {
    const id = modelDraft.trim()
    if (!id) { notify(t('modelIdRequired')); return }
    if (models.includes(id)) { notify(t('modelExists')); return }
    setModels((current) => [...current, id])
    setModelDraft('')
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
    }
    return results
  }
  const probeKey = async (index: number): Promise<KeyProbe> => {
    const key = keyDrafts[index]
    const secret = key.value.trim()
    const model = (key.models && key.models[0]) || key.detected?.[0] || models[0] || ''
    if (!model) throw new Error(t('keyTestNeedsModel'))
    const response = provider
      ? await run(() => api<{ model: string; result: Omit<KeyProbe, 'model'> }>(`/api/v1/providers/${provider.id}/probe`, { method: 'POST', body: JSON.stringify({ protocol: type, endpoint, secret: secret || undefined, key_index: secret ? undefined : index, model }) }))
      : await run(() => api<{ model: string; result: Omit<KeyProbe, 'model'> }>('/api/v1/providers/probe', { method: 'POST', body: JSON.stringify({ protocol: type, endpoint, secret, model }) }))
    return { ...response.result, model: response.model }
  }
  const testKeys = async () => {
    if (!endpoint.trim()) { notify(t('modelsRequired')); return }
    if (!provider && !keyDrafts.some((key) => key.value.trim())) { notify(t('apiKeyRequired')); return }
    setTesting(true)
    try {
      const results = await Promise.all(keyDrafts.map(async (key, index) => {
        try { return { id: key.id, probe: await probeKey(index), error: null as string | null } } catch (cause) { return { id: key.id, probe: null, error: cause instanceof Error ? cause.message : String(cause) } }
      }))
      setKeyDrafts((current) => current.map((key) => {
        const result = results.find((item) => item.id === key.id)
        if (!result) return key
        return result.probe ? { ...key, probe: result.probe, error: result.probe.ok ? null : result.probe.error } : { ...key, probe: null, error: result.error }
      }))
      const ok = results.filter((result) => result.probe?.ok).length
      setTested(ok > 0 && ok === results.length)
      notify(t('keyTestSummary', { ok, total: results.length }))
    } catch (cause) { setTested(false); notify(cause instanceof Error ? cause.message : String(cause)) } finally { setTesting(false) }
  }
  const clearModels = () => {
    setModels([])
    notify(t('modelsCleared'))
  }
  const addKey = () => setKeyDrafts((current) => [...current, createKeyDraft()])
  const updateKey = (id: number, value: string) => setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, value, models: null, detected: null, error: null, probe: null } : key))
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
  const busy = testing || detectingKey !== null
  const probeResults = keyDrafts.flatMap((key, index) => key.probe ? [{ key, index, probe: key.probe }] : [])
  const probePassed = probeResults.length > 0 && probeResults.every((item) => item.probe.ok)
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
          <div className="key-manager-heading"><span className="field-label">{t('apiKeys')}</span><div className="key-manager-actions"><button type="button" className="button text-button" disabled={busy || !endpoint.trim() || (!provider && !keyDrafts.some((key) => key.value.trim()))} onClick={testKeys}><Play size={12} />{testing ? t('keyTesting') : t('testKeys')}</button><button type="button" className="button text-button" onClick={addKey}><Plus size={12} />{t('addKey')}</button></div></div>
          <div className="key-input-list">{keyDrafts.map((key, index) => <div className="key-input-item" key={key.id}>
            <div className="key-input-row"><input className="field" type="password" value={key.value} onChange={(event) => updateKey(key.id, event.target.value)} placeholder={key.configured ? t('notStored') : t('apiKeyPlaceholder')} aria-label={t('apiKeyNumber', { number: index + 1 })} autoComplete="new-password" /><button type="button" className="button secondary key-detect-button" disabled={busy || !endpoint.trim() || (!provider && !key.value.trim())} onClick={() => detectKeyModels(key.id)}><RefreshCw className={detectingKey === key.id ? 'spin' : ''} size={12} />{t('detectKeyModels')}</button><button type="button" className="icon-button" disabled={keyDrafts.length === 1} onClick={() => removeKey(key.id)} aria-label={t('removeApiKey', { number: index + 1 })}><X size={14} /></button></div>
            {key.models === null
              ? <small className="field-hint">{key.configured && !key.value.trim() ? t('keyScopeStored') : t('keyScopeAll')}</small>
              : <small className="field-hint">{t('keyScopeCount', { count: key.models.length })} · <button type="button" className="button text-button key-scope-edit" onClick={() => setModelSettingsOpen(true)}>{t('keyModelsEdit')}</button></small>}
            {key.error && !key.probe && <small className="key-scope-error">{key.error}</small>}
          </div>)}</div>
          {probeResults.length > 0 && <div className="key-test-panel">
            <div className={`key-test-panel-head ${probePassed ? 'pass' : 'fail'}`}>{probePassed ? <Check size={14} /> : <X size={14} />}<strong>{probePassed ? t('keyTestPassed') : t('keyTestFailed')}</strong><small>{t('keyTestResultCount', { count: probeResults.length })}</small></div>
            {probeResults.map(({ key, index, probe }) => <div className="key-test-row" key={key.id}>
              <div className="key-test-row-top"><code>{`${endpoint.replace(/\/+$/, '')}/${probe.endpoint || ''}`}</code><span className="key-test-badge">{key.value.trim() ? maskKey(key.value.trim()) : t('apiKeyNumber', { number: index + 1 })}</span><span className="key-test-badge">{probe.model}</span><span className={`key-test-status ${probe.ok ? 'pass' : 'fail'}`}>{probe.ok ? <Check size={12} /> : <X size={12} />}{probe.latency_ms} ms · {probe.ok ? t('testPassed') : t('testFailed')}</span></div>
              <p className={`key-test-row-note ${probe.ok ? '' : 'error'}`}>{probe.ok ? (probe.reply || t('keyTestOk')) : probe.error}</p>
            </div>)}
          </div>}
          <small className="field-hint">{t('multipleKeysHint')} {t('keyScopeHint')}</small>
        </div>
        <Field label={t('baseUrl')} type="url" value={endpoint} onChange={setEndpoint} placeholder="https://api.example.com/v1" />
      </div>
      <section className="model-manager">
        <div className="channel-section-title"><span className="channel-section-icon"><Settings2 size={14} /></span><h3>{t('modelManagement')}</h3><span className="model-count">{models.length} {t('models')}</span></div>
        <div className="model-toolbar">
          <input className="field model-id-input" value={modelDraft} onChange={(event) => setModelDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addModel() } }} placeholder={t('modelIdPlaceholder')} aria-label={t('modelIdPlaceholder')} />
          <button type="button" className="button secondary" disabled={!modelDraft.trim() || busy} onClick={addModel}><Plus size={13} />{t('addModel')}</button>
          <button type="button" className="button secondary" onClick={() => setModelSettingsOpen(true)}><Settings2 size={13} />{t('keyModelsTitle')}</button>
          <button type="button" className="button secondary" disabled={!models.length} onClick={clearModels}>{t('clear')}</button>
        </div>
        <div className={`model-chip-list ${models.length ? '' : 'is-empty'}`}>
          {models.map((model) => <span className="model-chip" key={model}><code>{model}</code><button type="button" onClick={() => setModels((current) => current.filter((item) => item !== model))} aria-label={t('removeModel', { name: model })}><X size={12} /></button></span>)}
          {!models.length && <span>{t('noModelsConfigured')}</span>}
        </div>
      </section>
      <div className="switch-row"><span>{t('enableProvider')}</span><Toggle label={t('enableProvider')} checked={enabled} onChange={() => setEnabled(!enabled)} /></div>
      <DialogActions onClose={onClose} label={provider ? t('saveProvider') : t('connectProvider')} leading={<button type="button" className="button secondary dialog-actions-leading" disabled={busy || !endpoint.trim() || (!provider && !keyDrafts.some((key) => key.value.trim()))} onClick={async () => { setTesting(true); try { const results = await detectAllKeys(); setTested(results.some((result) => result.models?.length)); const failed = results.find((result) => result.error); if (failed?.error) notify(failed.error) } catch (cause) { setTested(false); notify(cause instanceof Error ? cause.message : String(cause)) } finally { setTesting(false) } }}>{testing ? t('testingConnection') : t('testConnection')}</button>} />
      {tested && <div className="provider-test-result"><Check size={13} />{t('configLooksGood')}</div>}
      {provider && onDelete && <button type="button" className="dialog-danger-link" onClick={onDelete}>{t('disconnectProvider')}</button>}
    </form>
  </Modal>}
  <Modal open={modelSettingsOpen} onClose={() => setModelSettingsOpen(false)} title={t('keyModelsTitle')} description={t('keyModelsSubtitle')} size="nested-dialog model-settings-dialog">
    <div className="model-settings-body">
      <div className="key-scope-sections">{keyDrafts.map((key, index) => <section className="key-scope-section" key={key.id}>
        <div className="key-scope-section-head"><strong>{t('apiKeyNumber', { number: index + 1 })}</strong><button type="button" className="button secondary key-detect-button" disabled={busy || !endpoint.trim() || (!provider && !key.value.trim())} onClick={() => detectKeyModels(key.id)}><RefreshCw className={detectingKey === key.id ? 'spin' : ''} size={12} />{t('detectKeyModels')}</button></div>
        {key.detected && <small className="field-hint">{t('keyModelsDetected', { number: index + 1, count: key.detected.length })}</small>}
        {key.error && <small className="key-scope-error">{key.error}</small>}
        {key.models === null
          ? <div className="key-scope-all-row"><small className="field-hint">{key.configured && !key.value.trim() ? t('keyScopeStored') : t('keyScopeAll')}</small>{!!models.length && <button type="button" className="button text-button" onClick={() => setKeyDrafts((current) => current.map((item) => item.id === key.id ? { ...item, models: [] } : item))}>{t('keyScopeLimit')}</button>}</div>
          : <><div className="key-scope-chips">{models.map((model) => <label className={`key-scope-chip ${key.models?.includes(model) ? 'selected' : ''}`} key={model}><input type="checkbox" checked={key.models?.includes(model) || false} onChange={() => toggleKeyModel(key.id, model)} /><code>{model}</code></label>)}<button type="button" className="button text-button" onClick={() => clearKeyScope(key.id)}>{t('keyScopeAll')}</button></div>{!models.length && <small className="field-hint">{t('noModelsConfigured')}</small>}</>}
      </section>)}</div>
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