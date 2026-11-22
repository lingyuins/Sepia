import { useEffect, useState } from 'react'
import { Check, Copy, Play, Plus, RefreshCw, Search, Settings2, X } from 'lucide-react'
import { DialogActions, Field, Modal, Toggle } from './UI'
import { Dropdown } from './Dropdown'
import { api, type ProviderKeyDraft } from '../api'
import { protocolOf, useTranslation, useWorkspace, type ChannelGroup, type ProtocolType, type Provider, type ProviderEndpoint } from '../workspace'
import { addSelectedModels, filterModels, mergeDetectedModels } from '../fetch-models'
export type { Provider } from '../workspace'
export type { ProviderKeyDraft } from '../api'

type KeyCheck = { ok: boolean; models: string[]; latency_ms: number; error: string | null }
type KeyDraft = { id: number; value: string; configured: boolean; models: string[] | null; error: string | null; check: KeyCheck | null; endpointId: number; storedId: number | null }
type EndpointDraft = { id: number; storedId: number | null; endpoint: string }
let keyDraftId = 0
let endpointDraftId = 0
const createKeyDraft = (endpointId: number, configured = false, models: string[] | null = null, storedId: number | null = null): KeyDraft => ({ id: ++keyDraftId, value: '', configured, models, error: null, check: null, endpointId, storedId })
const initialDrafts = (provider?: Provider | null): { endpoints: EndpointDraft[]; keys: KeyDraft[] } => {
  if (!provider) {
    const first = ++endpointDraftId
    return { endpoints: [{ id: first, storedId: null, endpoint: '' }], keys: [createKeyDraft(first)] }
  }
  const entries = provider.endpoints.length
    ? provider.endpoints
    : [{ id: -1, endpoint: provider.endpoint, keys: (provider.keyScopes || []).map((scope) => ({ id: scope.id, models: scope.models })) }]
  const endpoints: EndpointDraft[] = entries.map((entry) => ({ id: ++endpointDraftId, storedId: entry.id > 0 ? entry.id : null, endpoint: entry.endpoint }))
  const keys = entries.flatMap((entry, index) => {
    const local = endpoints[index].id
    if (entry.keys.length) return entry.keys.map((scope) => createKeyDraft(local, true, scope.models, scope.id > 0 ? scope.id : null))
    return Array.from({ length: Math.max(provider.keyCount || 1, 1) }, () => createKeyDraft(local, true))
  })
  return { endpoints, keys }
}
const maskKey = (value: string) => value.length > 12 ? `${value.slice(0, 4)}…${value.slice(-4)}` : '••••'

export function ProviderMark({ name, short, color, small = false }: Pick<Provider, 'name' | 'short' | 'color'> & { small?: boolean }) {
  return <span className={`provider-mark ${small ? 'small' : ''}`} style={{ background: `${color}14`, color }} aria-label={name}>{short}</span>
}

export function ProviderDialog({ open, onClose, onSave, onDelete, onTest, provider }: { open: boolean; onClose: () => void; onSave: (provider: Provider, keys: ProviderKeyDraft[]) => Promise<void>; onDelete?: () => void; onTest?: (provider: Provider, keyIndex?: number, secret?: string) => Promise<string[]>; provider?: Provider | null }) {
  const [name, setName] = useState('')
  const { channelGroups, setChannelGroups, notify } = useWorkspace()
  const t = useTranslation()
  const [type, setType] = useState<ProtocolType>('openai')
  const [group, setGroup] = useState('official')
  const [mode, setMode] = useState<'single' | 'unified'>('single')
  const [endpointDrafts, setEndpointDrafts] = useState<EndpointDraft[]>(() => initialDrafts(provider).endpoints)
  const [enabled, setEnabled] = useState(provider?.enabled ?? true)
  const [testing, setTesting] = useState(false)
  const [keyDrafts, setKeyDrafts] = useState<KeyDraft[]>(() => initialDrafts(provider).keys)
  const [groupOpen, setGroupOpen] = useState(false)
  const [groupName, setGroupName] = useState('')
  const [groupDescription, setGroupDescription] = useState('')
  const [models, setModels] = useState<string[]>(provider?.modelsList || [])
  const [modelDraft, setModelDraft] = useState('')
  const [modelSettingsOpen, setModelSettingsOpen] = useState(false)
  const [fetchOpen, setFetchOpen] = useState(false)
  const [fetchQuery, setFetchQuery] = useState('')
  const [fetching, setFetching] = useState(false)
  const [fetchResults, setFetchResults] = useState<{ keyId: number; models: string[] | null; error: string | null }[]>([])
  const [selectedModels, setSelectedModels] = useState<string[]>([])
  useEffect(() => {
    if (!open) return
    const nextType = provider ? protocolOf(provider) : 'openai'
    const drafts = initialDrafts(provider)
    setName(provider?.name || '')
    setType(nextType)
    setGroup(provider?.group || 'official')
    setMode(provider?.mode || 'single')
    setEndpointDrafts(drafts.endpoints)
    setEnabled(provider?.enabled ?? true)
    setKeyDrafts(drafts.keys)
    setModels([...(provider?.modelsList || [])])
    setModelDraft('')
    setModelSettingsOpen(false)
    setFetchOpen(false)
    setGroupOpen(false)
    setTesting(false)
  }, [open, provider])
  const firstEndpoint = endpointDrafts[0]
  const endpoint = firstEndpoint?.endpoint || ''
  const firstKeys = keyDrafts.filter((key) => key.endpointId === firstEndpoint?.id)
  const changeType = (value: string) => {
    setType(value as ProtocolType)
  }
  const addModel = () => {
    const id = modelDraft.trim()
    if (!id) { notify(t('modelIdRequired')); return }
    if (models.includes(id)) { notify(t('modelExists')); return }
    setModels((current) => [...current, id])
    setModelDraft('')
    notify(t('modelAdded', { name: id }))
  }
  // Detection always targets the first address: unified channels share one
  // model catalog, so every key of the first address is probed and later
  // addresses only participate in inference failover.
  const discoverKey = (key: KeyDraft, address: string, index: number) => {
    const secret = key.value.trim()
    if (provider && onTest && key.storedId !== null) return onTest({ ...provider, type, endpoint: address }, index, secret || undefined)
    if (!secret) return Promise.reject(new Error(t('apiKeyRequired')))
    return api<{ models: string[] }>('/api/v1/providers/test', {
      method: 'POST',
      body: JSON.stringify({ protocol: type, endpoint: address, secret }),
    }).then((result) => result.models)
  }
  const runFetch = async () => {
    setFetching(true)
    setFetchResults([])
    try {
      const results = await Promise.all(firstKeys.map(async (key, index) => {
        try { return { keyId: key.id, models: await discoverKey(key, endpoint, index), error: null as string | null } } catch (cause) { return { keyId: key.id, models: null, error: cause instanceof Error ? cause.message : String(cause) } }
      }))
      setFetchResults(results)
      const union = mergeDetectedModels(results)
      setSelectedModels(models.filter((model) => union.includes(model)))
      const failed = results.find((result) => result.error)
      if (failed?.error && !union.length) notify(failed.error)
    } finally { setFetching(false) }
  }
  const openFetch = () => {
    if (!endpoint.trim()) { notify(t('modelsRequired')); return }
    if (!provider && !firstKeys.some((key) => key.value.trim())) { notify(t('apiKeyRequired')); return }
    setFetchQuery('')
    setFetchOpen(true)
    void runFetch()
  }
  const applyFetch = () => {
    if (!selectedModels.length) return
    setModels((current) => addSelectedModels(current, selectedModels))
    notify(t('modelsAddedCount', { count: selectedModels.length }))
    setFetchOpen(false)
  }
  const toggleSelected = (model: string) => setSelectedModels((current) => current.includes(model) ? current.filter((item) => item !== model) : [...current, model])
  const testKeys = async () => {
    if (!endpoint.trim()) { notify(t('modelsRequired')); return }
    if (!provider && !keyDrafts.some((key) => key.value.trim())) { notify(t('apiKeyRequired')); return }
    setTesting(true)
    try {
      const results = await Promise.all(keyDrafts.map(async (key) => {
        const address = endpointDrafts.find((entry) => entry.id === key.endpointId)
        const index = keyDrafts.filter((item) => item.endpointId === key.endpointId).findIndex((item) => item.id === key.id)
        const started = Date.now()
        try {
          const discovered = await discoverKey(key, address?.endpoint || endpoint, index)
          const check: KeyCheck = { ok: discovered.length > 0, models: discovered, latency_ms: Date.now() - started, error: discovered.length ? null : t('keyTestNoModels') }
          return { id: key.id, endpoint: address?.endpoint || '', check }
        } catch (cause) {
          return { id: key.id, endpoint: address?.endpoint || '', check: { ok: false, models: [], latency_ms: Date.now() - started, error: cause instanceof Error ? cause.message : String(cause) } }
        }
      }))
      setKeyDrafts((current) => current.map((key) => {
        const result = results.find((item) => item.id === key.id)
        if (!result) return key
        return { ...key, check: result.check, error: result.check.error }
      }))
      const ok = results.filter((result) => result.check.ok).length
      notify(t('keyTestSummary', { ok, total: results.length }))
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setTesting(false) }
  }
  const clearModels = () => {
    setModels([])
    notify(t('modelsCleared'))
  }
  const addKey = (endpointId: number) => setKeyDrafts((current) => [...current, createKeyDraft(endpointId)])
  const updateKey = (id: number, value: string) => setKeyDrafts((current) => current.map((key) => key.id === id ? { ...key, value, models: null, error: null, check: null } : key))
  const removeKey = (id: number) => setKeyDrafts((current) => {
    const key = current.find((item) => item.id === id)
    if (!key || current.filter((item) => item.endpointId === key.endpointId).length <= 1) return current
    return current.filter((item) => item.id !== id)
  })
  const addEndpoint = () => {
    const id = ++endpointDraftId
    setEndpointDrafts((current) => [...current, { id, storedId: null, endpoint: '' }])
    setKeyDrafts((current) => [...current, createKeyDraft(id)])
  }
  const updateEndpoint = (id: number, value: string) => setEndpointDrafts((current) => current.map((entry) => entry.id === id ? { ...entry, endpoint: value } : entry))
  const removeEndpoint = (id: number) => {
    setEndpointDrafts((current) => current.length > 1 ? current.filter((entry) => entry.id !== id) : current)
    setKeyDrafts((current) => current.filter((key) => key.endpointId !== id))
  }
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
    if (mode === 'unified' && endpointDrafts.some((entry) => !entry.endpoint.trim())) { notify(t('endpointRequired')); return }
    if (!models.length) { notify(t('modelsRequired')); return }
    try {
      const keyCount = keyDrafts.filter((key) => key.configured || key.value.trim()).length
      const endpoints: ProviderEndpoint[] = endpointDrafts.map((entry) => ({ id: entry.storedId ?? entry.id, endpoint: entry.endpoint.trim(), keys: keyDrafts.filter((key) => key.endpointId === entry.id).map((key) => ({ id: key.storedId ?? 0, models: key.models, endpointId: entry.storedId ?? entry.id })) }))
      const saved: Provider = { id: provider?.id || Date.now(), name: name.trim(), short: name.trim().charAt(0).toUpperCase(), type, group, models: models.length, latency: provider?.latency || 0, success: provider?.success || '—', requests: provider?.requests || '0', color: provider?.color || '#c4f4aa', enabled, endpoint: endpoint.trim(), mode, endpoints, modelsList: models, keyCount, keyScopes: provider?.keyScopes }
      await onSave(saved, keyDrafts.filter((key) => key.configured || key.value.trim()).map((key) => ({ value: key.value.trim(), models: key.models, endpointId: endpointDrafts.find((entry) => entry.id === key.endpointId)?.storedId ?? endpointDrafts.find((entry) => entry.id === key.endpointId)?.id })))
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const createGroup = async (event: React.FormEvent) => {
    event.preventDefault()
    const name = groupName.trim()
    if (!name) { notify(t('groupNameRequired')); return }
    if (channelGroups.some((item) => item.name.toLowerCase() === name.toLowerCase())) { notify(t('groupExists')); return }
    const id = `group-${Date.now()}`
    try {
      await api('/api/v1/groups', {
        method: 'POST',
        body: JSON.stringify({ id, name, description: groupDescription.trim() }),
      })
      const created: ChannelGroup = { id, name, description: groupDescription.trim() }
      setChannelGroups((current) => [...current, created])
      setGroup(created.id)
      setGroupOpen(false)
      setGroupName('')
      setGroupDescription('')
      notify(t('groupCreated'))
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    }
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
  const busy = testing || fetching
  const checkResults = keyDrafts.flatMap((key) => key.check ? [{ key, check: key.check, endpoint: endpointDrafts.find((entry) => entry.id === key.endpointId)?.endpoint || '' }] : [])
  const checkPassed = checkResults.length > 0 && checkResults.every((item) => item.check.ok)
  const fetchUnion = mergeDetectedModels(fetchResults)
  const visibleUnion = filterModels(fetchUnion, fetchQuery)
  const renderKeys = (endpointId: number) => {
    const keys = keyDrafts.filter((key) => key.endpointId === endpointId)
    return <div className="key-input-list">{keys.map((key, index) => <div className="key-input-item" key={key.id}>
      <div className="key-input-row"><input className="field" type="password" value={key.value} onChange={(event) => updateKey(key.id, event.target.value)} placeholder={key.configured ? t('notStored') : t('apiKeyPlaceholder')} aria-label={t('apiKeyNumber', { number: index + 1 })} autoComplete="new-password" /><button type="button" className="icon-button" disabled={keys.length === 1} onClick={() => removeKey(key.id)} aria-label={t('removeApiKey', { number: index + 1 })}><X size={14} /></button></div>
      {key.models === null
        ? <small className="field-hint">{key.configured && !key.value.trim() ? t('keyScopeStored') : t('keyScopeAll')}</small>
        : <small className="field-hint">{t('keyScopeCount', { count: key.models.length })} · <button type="button" className="button text-button key-scope-edit" onClick={() => setModelSettingsOpen(true)}>{t('keyModelsEdit')}</button></small>}
      {key.error && !key.check && <small className="key-scope-error">{key.error}</small>}
    </div>)}</div>
  }
  return <>{!groupOpen && !modelSettingsOpen && !fetchOpen && <Modal open={open} onClose={onClose} title={provider ? t('manageProvider', { name: provider.name }) : t('connectProviderTitle')} description={t('providerDialogSubtitle')} size="channel-dialog">
    <form onSubmit={submit}>
      <div className="channel-section-title"><span className="channel-section-icon"><Check size={14} /></span><h3>{t('basicInformation')}</h3></div>
      <div className="channel-fields">
        <Field label={t('channelName')} value={name} onChange={setName} required placeholder={t('channelNamePlaceholder')} />
        <Dropdown label={t('providerType')} value={type} options={protocolOptions} onChange={changeType} />
        <Dropdown label={t('channelGroup')} value={group} options={channelGroups.map((item) => ({ value: item.id, label: groupLabel(item.id), description: item.id === 'official' ? t('officialGroupHint') : item.id === 'free' ? t('freeGroupHint') : item.id === 'team' ? t('teamGroupHint') : item.description }))} onChange={setGroup} onCreate={() => setGroupOpen(true)} />
        <Dropdown label={t('channelMode')} value={mode} options={[{ value: 'single', label: t('singleChannel'), description: t('singleChannelHint') }, { value: 'unified', label: t('unifiedChannel'), description: t('unifiedChannelHint') }]} onChange={(value) => setMode(value as 'single' | 'unified')} />
      </div>
      {mode === 'single' && <div className="channel-connection-fields">
        <div className="key-manager">
          <div className="key-manager-heading"><span className="field-label">{t('apiKeys')}</span><div className="key-manager-actions"><button type="button" className="button text-button" disabled={busy || !endpoint.trim() || (!provider && !keyDrafts.some((key) => key.value.trim()))} onClick={testKeys}><Play size={12} />{testing ? t('keyTesting') : t('testKeys')}</button><button type="button" className="button text-button" onClick={() => addKey(firstEndpoint.id)}><Plus size={12} />{t('addKey')}</button></div></div>
          {renderKeys(firstEndpoint.id)}
          {checkResults.length > 0 && <div className="key-test-panel">
            <div className={`key-test-panel-head ${checkPassed ? 'pass' : 'fail'}`}>{checkPassed ? <Check size={14} /> : <X size={14} />}<strong>{checkPassed ? t('keyTestPassed') : t('keyTestFailed')}</strong><small>{t('keyTestResultCount', { count: checkResults.length })}</small></div>
            {checkResults.map(({ key, check, endpoint: address }) => <div className="key-test-row" key={key.id}>
              <div className="key-test-row-top"><code>{address.replace(/\/+$/, '')}</code><span className="key-test-badge">{key.value.trim() ? maskKey(key.value.trim()) : t('apiKeyNumber', { number: firstKeys.findIndex((item) => item.id === key.id) + 1 })}</span><span className={`key-test-status ${check.ok ? 'pass' : 'fail'}`}>{check.ok ? <Check size={12} /> : <X size={12} />}{check.latency_ms} ms · {check.ok ? t('testPassed') : t('testFailed')}</span></div>
              <p className={`key-test-row-note ${check.ok ? '' : 'error'}`}>{check.ok ? t('keyTestCatalogOk', { count: check.models.length }) : check.error}</p>
            </div>)}
          </div>}
          <small className="field-hint">{t('multipleKeysHint')} {t('keyScopeHint')}</small>
        </div>
        <Field label={t('baseUrl')} type="url" value={firstEndpoint.endpoint} onChange={(value) => updateEndpoint(firstEndpoint.id, value)} placeholder="https://api.example.com/v1" />
      </div>}
      {mode === 'unified' && <div className="unified-endpoints">
        <div className="key-manager-heading"><span className="field-label">{t('unifiedAddresses')}</span><div className="key-manager-actions"><button type="button" className="button text-button" disabled={busy || !endpoint.trim() || (!provider && !firstKeys.some((key) => key.value.trim()))} onClick={testKeys}><Play size={12} />{testing ? t('keyTesting') : t('testKeys')}</button><button type="button" className="button text-button" onClick={addEndpoint}><Plus size={12} />{t('addEndpoint')}</button></div></div>
        <small className="field-hint">{t('unifiedChannelHint')}</small>
        {endpointDrafts.map((entry, addressIndex) => <section className="unified-endpoint-card" key={entry.id}>
          <div className="unified-endpoint-head"><strong>{t('endpointNumber', { number: addressIndex + 1 })}</strong><button type="button" className="icon-button" disabled={endpointDrafts.length === 1} onClick={() => removeEndpoint(entry.id)} aria-label={t('removeEndpoint', { number: addressIndex + 1 })}><X size={14} /></button></div>
          <Field label={t('baseUrl')} type="url" value={entry.endpoint} onChange={(value) => updateEndpoint(entry.id, value)} placeholder="https://api.example.com/v1" />
          <div className="key-manager-heading"><span className="field-label">{t('apiKeys')}</span><button type="button" className="button text-button" onClick={() => addKey(entry.id)}><Plus size={12} />{t('addKey')}</button></div>
          {renderKeys(entry.id)}
        </section>)}
        {checkResults.length > 0 && <div className="key-test-panel">
          <div className={`key-test-panel-head ${checkPassed ? 'pass' : 'fail'}`}>{checkPassed ? <Check size={14} /> : <X size={14} />}<strong>{checkPassed ? t('keyTestPassed') : t('keyTestFailed')}</strong><small>{t('keyTestResultCount', { count: checkResults.length })}</small></div>
          {checkResults.map(({ key, check, endpoint: address }) => <div className="key-test-row" key={key.id}>
            <div className="key-test-row-top"><code>{address.replace(/\/+$/, '')}</code><span className="key-test-badge">{key.value.trim() ? maskKey(key.value.trim()) : t('apiKeyNumber', { number: keyDrafts.filter((item) => item.endpointId === key.endpointId).findIndex((item) => item.id === key.id) + 1 })}</span><span className={`key-test-status ${check.ok ? 'pass' : 'fail'}`}>{check.ok ? <Check size={12} /> : <X size={12} />}{check.latency_ms} ms · {check.ok ? t('testPassed') : t('testFailed')}</span></div>
            <p className={`key-test-row-note ${check.ok ? '' : 'error'}`}>{check.ok ? t('keyTestCatalogOk', { count: check.models.length }) : check.error}</p>
          </div>)}
        </div>}
        <small className="field-hint">{t('multipleKeysHint')} {t('keyScopeHint')}</small>
      </div>}
      <section className="model-manager">
        <div className="channel-section-title"><span className="channel-section-icon"><Settings2 size={14} /></span><h3>{t('modelManagement')}</h3><span className="model-count">{models.length} {t('models')}</span></div>
        <div className="model-toolbar">
          <input className="field model-id-input" value={modelDraft} onChange={(event) => setModelDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addModel() } }} placeholder={t('modelIdPlaceholder')} aria-label={t('modelIdPlaceholder')} />
          <button type="button" className="button secondary" disabled={!modelDraft.trim() || busy} onClick={addModel}><Plus size={13} />{t('addModel')}</button>
          <button type="button" className="button secondary" disabled={busy} onClick={openFetch}><RefreshCw className={fetching ? 'spin' : ''} size={13} />{t('fetchModels')}</button>
          <button type="button" className="button secondary" onClick={() => setModelSettingsOpen(true)}><Settings2 size={13} />{t('keyModelsTitle')}</button>
          <button type="button" className="button secondary" disabled={!models.length} onClick={clearModels}>{t('clear')}</button>
        </div>
        <div className={`model-chip-list ${models.length ? '' : 'is-empty'}`}>
          {models.map((model) => <span className="model-chip" key={model}><code>{model}</code><button type="button" onClick={() => setModels((current) => current.filter((item) => item !== model))} aria-label={t('removeModel', { name: model })}><X size={12} /></button></span>)}
          {!models.length && <span>{t('noModelsConfigured')}</span>}
        </div>
      </section>
      <div className="switch-row"><span>{t('enableProvider')}</span><Toggle label={t('enableProvider')} checked={enabled} onChange={() => setEnabled(!enabled)} /></div>
      <DialogActions onClose={onClose} label={provider ? t('saveProvider') : t('connectProvider')} disabled={busy} />
      {provider && onDelete && <button type="button" className="dialog-danger-link" onClick={onDelete}>{t('disconnectProvider')}</button>}
    </form>
  </Modal>}
  <Modal open={fetchOpen} onClose={() => setFetchOpen(false)} title={t('fetchModels')} description={t('fetchModelsHint')} size="nested-dialog fetch-models-dialog">
    <div className="fetch-models-body">
      <label className="form-field fetch-models-search"><span className="field-label">{t('searchModels')}</span><span className="fetch-search-input"><Search size={14} /><input className="field" value={fetchQuery} onChange={(event) => setFetchQuery(event.target.value)} placeholder={t('searchModels')} aria-label={t('searchModels')} /></span></label>
      {fetching && <div className="fetch-models-loading"><RefreshCw className="spin" size={14} />{t('fetchingModels')}</div>}
      {!fetching && <section className="fetch-models-section">
        <div className="fetch-models-section-head"><strong>{t('allModels')}</strong><small>{t('modelsCount', { count: fetchUnion.length })}</small>{!!visibleUnion.length && <button type="button" className="button text-button" onClick={() => setSelectedModels(visibleUnion.every((model) => selectedModels.includes(model)) ? selectedModels.filter((model) => !visibleUnion.includes(model)) : [...new Set([...selectedModels, ...visibleUnion])])}>{visibleUnion.every((model) => selectedModels.includes(model)) ? t('clearSelection') : t('selectAll')}</button>}</div>
        <div className="fetch-models-list">
          {visibleUnion.map((model) => <label className={`fetch-model-item ${selectedModels.includes(model) ? 'selected' : ''}`} key={model}><input type="checkbox" checked={selectedModels.includes(model)} onChange={() => toggleSelected(model)} /><code>{model}</code>{models.includes(model) && <small>{t('alreadyConfigured')}</small>}</label>)}
          {!visibleUnion.length && <span className="fetch-models-empty">{fetchUnion.length ? t('noMatchingModels') : t('noModelsDetected')}</span>}
        </div>
      </section>}
      {!fetching && fetchResults.map((result) => {
        const key = firstKeys.find((item) => item.id === result.keyId)
        const index = firstKeys.findIndex((item) => item.id === result.keyId)
        const visible = filterModels(result.models || [], fetchQuery)
        return <section className="fetch-models-section" key={result.keyId}>
          <div className="fetch-models-section-head"><strong>{t('apiKeyNumber', { number: index + 1 })}</strong><small>{key?.value.trim() ? maskKey(key.value.trim()) : t('notStored')}</small></div>
          {result.error && <p className="key-scope-error">{result.error}</p>}
          <div className="fetch-models-list">
            {visible.map((model) => <label className={`fetch-model-item ${selectedModels.includes(model) ? 'selected' : ''}`} key={model}><input type="checkbox" checked={selectedModels.includes(model)} onChange={() => toggleSelected(model)} /><code>{model}</code></label>)}
            {!visible.length && !result.error && <span className="fetch-models-empty">{result.models?.length ? t('noMatchingModels') : t('keyTestNoModels')}</span>}
          </div>
        </section>
      })}
    </div>
    <div className="dialog-actions"><button type="button" className="button secondary" onClick={() => setFetchOpen(false)}>{t('cancel')}</button><button type="button" className="button primary" disabled={!selectedModels.length} onClick={applyFetch}>{t('addSelected', { count: selectedModels.length })}</button></div>
  </Modal>
  <Modal open={modelSettingsOpen} onClose={() => setModelSettingsOpen(false)} title={t('keyModelsTitle')} description={t('keyModelsSubtitle')} size="nested-dialog model-settings-dialog">
    <div className="model-settings-body">
      <div className="key-scope-sections">{keyDrafts.map((key) => {
        const addressIndex = endpointDrafts.findIndex((entry) => entry.id === key.endpointId)
        const index = keyDrafts.filter((item) => item.endpointId === key.endpointId).findIndex((item) => item.id === key.id)
        return <section className="key-scope-section" key={key.id}>
        <div className="key-scope-section-head"><strong>{mode === 'unified' ? `${t('endpointNumber', { number: addressIndex + 1 })} · ${t('apiKeyNumber', { number: index + 1 })}` : t('apiKeyNumber', { number: index + 1 })}</strong></div>
        {key.error && <small className="key-scope-error">{key.error}</small>}
        {key.models === null
          ? <div className="key-scope-all-row"><small className="field-hint">{key.configured && !key.value.trim() ? t('keyScopeStored') : t('keyScopeAll')}</small>{!!models.length && <button type="button" className="button text-button" onClick={() => setKeyDrafts((current) => current.map((item) => item.id === key.id ? { ...item, models: [] } : item))}>{t('keyScopeLimit')}</button>}</div>
          : <><div className="key-scope-chips">{models.map((model) => <label className={`key-scope-chip ${key.models?.includes(model) ? 'selected' : ''}`} key={model}><input type="checkbox" checked={key.models?.includes(model) || false} onChange={() => toggleKeyModel(key.id, model)} /><code>{model}</code></label>)}<button type="button" className="button text-button" onClick={() => clearKeyScope(key.id)}>{t('keyScopeAll')}</button></div>{!models.length && <small className="field-hint">{t('noModelsConfigured')}</small>}</>}
      </section>})}</div>
    </div>
    <div className="dialog-actions"><button type="button" className="button primary" onClick={() => setModelSettingsOpen(false)}>{t('done')}</button></div>
  </Modal>
  <Modal open={groupOpen} onClose={() => setGroupOpen(false)} title={t('createGroup')} description={t('groupDescriptionPlaceholder')} size="nested-dialog">
    <form onSubmit={createGroup}><Field label={t('groupName')} value={groupName} onChange={setGroupName} required placeholder={t('groupNamePlaceholder')} /><Field label={t('groupDescription')} value={groupDescription} onChange={setGroupDescription} placeholder={t('groupDescriptionPlaceholder')} /><DialogActions onClose={() => setGroupOpen(false)} label={t('createGroup')} /></form>
  </Modal>
  </>
}

export function CopyButton({ value, onCopy }: { value: string; onCopy: () => void }) {
  const t = useTranslation()
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try { await navigator.clipboard.writeText(value) } catch { const input = document.createElement('textarea'); input.value = value; document.body.appendChild(input); input.select(); document.execCommand('copy'); input.remove() }
    setCopied(true); onCopy(); setTimeout(() => setCopied(false), 1800)
  }
  return <button className="icon-button" onClick={copy} aria-label={t('copyClipboard')}>{copied ? <Check size={15} /> : <Copy size={15} />}</button>
}
