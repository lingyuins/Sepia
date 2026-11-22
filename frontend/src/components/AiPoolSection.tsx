import { useEffect, useState } from 'react'
import { Bot, LoaderCircle, Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react'
import { api } from '../api'
import { useWorkspace, useTranslation } from '../workspace'
import { DialogActions, Field, Modal, Toggle } from './UI'
import { Dropdown } from './Dropdown'
import type { TranslationKey } from '../i18n'
import { aiServiceInput, emptyAiServiceDraft, mapAiPoolService, serviceDraftFrom, validateAiServiceDraft, type AiPoolService, type AiPoolServiceDraft, type ApiAiPoolService } from '../ai-pool'

function ServiceForm({ service, onClose, onSaved }: { service: AiPoolService | null; onClose: () => void; onSaved: () => Promise<void> }) {
  const { notify, providers } = useWorkspace()
  const t = useTranslation()
  const [draft, setDraft] = useState<AiPoolServiceDraft>(() => service ? serviceDraftFrom(service) : { ...emptyAiServiceDraft })
  const [detecting, setDetecting] = useState(false)
  const [detectError, setDetectError] = useState('')
  const [busy, setBusy] = useState(false)
  const update = <K extends keyof AiPoolServiceDraft>(key: K, value: AiPoolServiceDraft[K]) => setDraft((current) => ({ ...current, [key]: value }))
  const providerBacked = draft.providerId !== null
  const sourceOptions = [
    { value: 'custom', label: t('aiServiceSourceCustom') },
    ...providers.filter((provider) => provider.enabled).map((provider) => ({ value: String(provider.id), label: provider.name })),
  ]
  const selectSource = (value: string) => {
    if (value === 'custom') { update('providerId', null); return }
    const provider = providers.find((item) => String(item.id) === value)
    if (!provider) return
    setDraft((current) => ({ ...current, providerId: provider.id, endpoint: provider.endpoint, secret: '', models: provider.modelsList, model: provider.modelsList.includes(current.model) ? current.model : provider.modelsList[0] || '' }))
  }
  const detect = async () => {
    setDetecting(true); setDetectError('')
    try {
      const path = service ? `/api/v1/ai/pool/${service.id}/test` : '/api/v1/ai/pool/test'
      const body = providerBacked ? { provider_id: draft.providerId } : { endpoint: draft.endpoint.trim(), secret: draft.secret.trim() }
      const { models } = await api<{ models: string[] }>(path, { method: 'POST', body: JSON.stringify(body) })
      setDraft((current) => ({ ...current, models, model: models.includes(current.model) ? current.model : models[0] || '' }))
      notify(t('keyTestCatalogOk', { count: models.length }))
    } catch (cause) { setDetectError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setDetecting(false) }
  }
  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const invalid = validateAiServiceDraft(draft, !!service)
    if (invalid) { notify(t(invalid.key as TranslationKey)); return }
    setBusy(true)
    try {
      await api(service ? `/api/v1/ai/pool/${service.id}` : '/api/v1/ai/pool', { method: service ? 'PUT' : 'POST', body: JSON.stringify(aiServiceInput(draft)) })
      await onSaved()
      notify(t(service ? 'aiServiceUpdated' : 'aiServiceAdded'))
      onClose()
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
    finally { setBusy(false) }
  }
  return <form className="ai-service-form" onSubmit={save}>
    <Field label={t('aiServiceName')} value={draft.name} onChange={(v) => update('name', v)} placeholder="OpenRouter" required />
    <Dropdown label={t('aiServiceSource')} value={draft.providerId === null ? 'custom' : String(draft.providerId)} options={sourceOptions} onChange={selectSource} />
    <small className="field-hint">{t('aiServiceSourceHint')}</small>
    {!providerBacked && <Field label={t('baseUrl')} value={draft.endpoint} onChange={(v) => update('endpoint', v)} placeholder="https://openrouter.ai/api/v1" required />}
    {!providerBacked && <Field label={t('apiKey')} value={draft.secret} onChange={(v) => update('secret', v)} placeholder={service ? t('aiServiceKeyKept') : 'sk-…'} required={!service} hint={service ? t('aiServiceKeyKept') : undefined} />}
    <div className="ai-service-model-row">
      <Dropdown label={t('aiServiceModel')} value={draft.model} options={draft.models.map((model) => ({ value: model, label: model }))} onChange={(v) => update('model', v)} placeholder={t('aiServiceModelPlaceholder')} />
      <button type="button" className="button secondary" onClick={() => void detect()} disabled={detecting || (!providerBacked && (!draft.endpoint.trim() || (!service && !draft.secret.trim())))}>{detecting ? <LoaderCircle className="gateway-setup-spinner" size={14} /> : <RefreshCw size={14} />}{t('detectKeyModels')}</button>
    </div>
    {detectError && <p className="route-test-error" role="alert">{detectError}</p>}
    <div className="ai-service-limits">
      <Field label={t('aiServiceTimeout')} value={draft.timeoutSeconds} onChange={(v) => update('timeoutSeconds', v)} type="number" hint={t('aiServiceTimeoutHint')} />
      <Field label={t('aiServiceConcurrency')} value={draft.concurrency} onChange={(v) => update('concurrency', v)} type="number" hint={t('aiServiceConcurrencyHint')} />
    </div>
    <div className="setting-row"><div><h4>{t('enabled')}</h4><p>{t('aiServiceEnabledHint')}</p></div><div className="setting-control"><Toggle checked={draft.enabled} onChange={() => update('enabled', !draft.enabled)} label={t('enabled')} /></div></div>
    <DialogActions onClose={onClose} label={t(service ? 'saveChanges' : 'aiServiceAdd')} disabled={busy} />
  </form>
}

export function AiPoolSection() {
  const { notify, providers } = useWorkspace()
  const t = useTranslation()
  const [services, setServices] = useState<AiPoolService[]>([])
  const [editing, setEditing] = useState<AiPoolService | null>(null)
  const [formOpen, setFormOpen] = useState(false)
  const load = async () => {
    try { setServices((await api<{ services: ApiAiPoolService[] }>('/api/v1/ai/pool')).services.map(mapAiPoolService)) }
    catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  useEffect(() => { void load() }, [])
  const toggle = async (service: AiPoolService) => {
    try {
      await api(`/api/v1/ai/pool/${service.id}`, { method: 'PUT', body: JSON.stringify(aiServiceInput({ ...serviceDraftFrom(service), enabled: !service.enabled })) })
      await load()
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const remove = async (service: AiPoolService) => {
    try { await api(`/api/v1/ai/pool/${service.id}`, { method: 'DELETE' }); await load(); notify(t('aiServiceRemoved')) }
    catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  return <section className="settings-card glass-panel">
    <div className="settings-card-title"><div><h3>{t('aiPoolServices')}</h3><p>{t('aiPoolServicesDescription')}</p></div><button type="button" className="button secondary" onClick={() => { setEditing(null); setFormOpen(true) }}><Plus size={14} />{t('aiServiceAdd')}</button></div>
    {services.map((service) => <div className="api-key-row" key={service.id}>
      <span className="key-icon"><Bot size={17} /></span>
      <div><strong>{service.name}</strong><span>{service.model} · {service.providerId !== null ? t('aiServiceFromProvider', { name: providers.find((provider) => provider.id === service.providerId)?.name || String(service.providerId) }) : service.endpoint} · {t('aiServiceLimits', { timeout: service.timeoutSeconds, concurrency: service.concurrency })}</span></div>
      <Toggle checked={service.enabled} onChange={() => void toggle(service)} label={t('toggleNamed', { name: service.name })} />
      <button type="button" className="icon-button" onClick={() => { setEditing(service); setFormOpen(true) }} aria-label={t('editNamed', { name: service.name })}><Pencil size={15} /></button>
      <button type="button" className="icon-button danger-icon" onClick={() => void remove(service)} aria-label={t('deleteNamed', { name: service.name })}><Trash2 size={15} /></button>
    </div>)}
    {services.length === 0 && <div className="empty-state">{t('aiPoolEmpty')}</div>}
    <Modal open={formOpen} onClose={() => setFormOpen(false)} title={t(editing ? 'aiServiceEdit' : 'aiServiceAdd')} description={t('aiServiceDialogSubtitle')}>
      <ServiceForm key={editing?.id || 'new'} service={editing} onClose={() => setFormOpen(false)} onSaved={load} />
    </Modal>
  </section>
}
