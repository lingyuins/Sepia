import { useState } from 'react'
import { Check, Plus, ShieldCheck, X } from 'lucide-react'
import { parseKeyPolicy, type KeyPolicy, type KeyPolicyDraft } from '../key-policy'
import { scopedModels } from '../key-models'
import { useTranslation, useWorkspace } from '../workspace'
import { DialogActions, Modal, Toggle } from './UI'

export type PreviewKey = KeyPolicy & { id: number; value: string; created: string }
function KeyField({ label, value, onChange, placeholder = '', type = 'text', hint, required = false }: { label: string; value: string; onChange: (value: string) => void; placeholder?: string; type?: string; hint?: string; required?: boolean }) {
  return <label className="key-policy-field"><span>{label}</span><input className="field" type={type} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} required={required} min={type === 'number' ? '0' : undefined} step={type === 'number' ? 'any' : undefined} />{hint && <small>{hint}</small>}</label>
}

export default function KeyPolicyDialog({ open, onClose, onCreate }: { open: boolean; onClose: () => void; onCreate: (policy: KeyPolicy) => Promise<void> }) {
  const { providers, channelGroups, routes } = useWorkspace()
  const t = useTranslation()
  const modelOptions = scopedModels(providers, routes)
  const models = modelOptions.map((model) => model.value)
  const [draft, setDraft] = useState<KeyPolicyDraft>(() => ({ name: '', customValue: '', tags: [], maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', models, allowedGroups: [], excludedProviders: [], enabled: true }))
  const [tagDraft, setTagDraft] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const update = <K extends keyof KeyPolicyDraft>(key: K, value: KeyPolicyDraft[K]) => { setDraft((current) => ({ ...current, [key]: value })); setError('') }
  const toggleListItem = (key: 'models' | 'allowedGroups' | 'excludedProviders', value: string) => update(key, draft[key].includes(value) ? draft[key].filter((item) => item !== value) : [...draft[key], value])
  const addTag = (value: string) => { const tag = value.trim(); if (tag && !draft.tags.includes(tag)) update('tags', [...draft.tags, tag]); setTagDraft('') }
  const close = () => { setError(''); onClose() }
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (busy) return
    try {
      const policy = parseKeyPolicy(draft)
      if (!policy.name) { setError(t('keyPolicyName')); return }
      setBusy(true)
      try { await onCreate(policy); setDraft({ name: '', customValue: '', tags: [], maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', models, allowedGroups: [], excludedProviders: [], enabled: true }) }
      catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
      finally { setBusy(false) }
    } catch { setError(t('keyPolicyError')) }
  }
  const groupLabel = (id: string, fallback: string) => id === 'official' ? t('officialChannels') : id === 'free' ? t('freeChannels') : id === 'team' ? t('teamChannels') : fallback

  return <Modal open={open} onClose={close} title={t('newKeyTitle')} description={t('newKeyDescription')} size="key-policy-dialog">
    <form className="key-policy-form" onSubmit={submit}>
      <div className="key-policy-scroll">
        <section className="key-policy-section">
          <h3>{t('keyIdentity')}</h3>
          <div className="key-policy-grid">
            <KeyField label={t('keyPolicyName')} value={draft.name} onChange={(value) => update('name', value)} placeholder={t('keyPolicyNamePlaceholder')} required />
          </div>
          <fieldset className="key-policy-field key-tags-field">
            <legend>{t('keyTags')}</legend>
            <div className="key-tag-list">
              {draft.tags.map((tag) => (
                <span className="key-tag" key={tag}>
                  {tag}
                  <button type="button" onClick={() => update('tags', draft.tags.filter((item) => item !== tag))} aria-label={`${t('keyTags')}: ${tag}`}><X size={11} /></button>
                </span>
              ))}
            </div>
            <div className="key-tag-input">
              <input className="field" value={tagDraft} onChange={(event) => setTagDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addTag(tagDraft) } }} placeholder={t('keyTagPlaceholder')} />
              <button type="button" className="button secondary" onClick={() => addTag(tagDraft)}><Plus size={13} />{t('keyAddTag')}</button>
            </div>
          </fieldset>
        </section>
        <section className="key-policy-section">
          <h3>{t('keyLimits')}</h3>
          <div className="key-policy-grid">
            <KeyField label={t('keyMaxAmount')} value={draft.maxAmount} onChange={(value) => update('maxAmount', value)} placeholder={t('keyAmountPlaceholder')} type="number" hint={t('keyUnlimited')} />
            <KeyField label={t('keyTokenLimit')} value={draft.tokenLimit} onChange={(value) => update('tokenLimit', value)} placeholder={t('keyTokenPlaceholder')} hint={t('keyUnlimited')} />
            <KeyField label={t('keyRpm')} value={draft.rpm} onChange={(value) => update('rpm', value)} type="number" />
            <KeyField label={t('keyTpm')} value={draft.tpm} onChange={(value) => update('tpm', value)} type="number" />
          </div>
          <label className="key-policy-field">
            <span>{t('keyQuotas')}</span>
            <textarea className="field key-quota-input" value={draft.modelQuotas} placeholder={'{"gpt-4o":{"rpm":5,"tpm":50000}}'} onChange={(event) => update('modelQuotas', event.target.value)} spellCheck={false} rows={3} />
          </label>
          <KeyField label={t('keyIpWhitelist')} value={draft.ipWhitelist} onChange={(value) => update('ipWhitelist', value)} placeholder={t('keyIpPlaceholder')} />
          <div className="key-policy-grid">
            <KeyField label={t('keyExpiry')} value={draft.expiresAt} onChange={(value) => update('expiresAt', value)} type="datetime-local" />
            <button type="button" className="key-never-expiry" onClick={() => update('expiresAt', '')}><ShieldCheck size={15} />{t('keyNever')}{!draft.expiresAt && <Check size={14} />}</button>
          </div>
        </section>
        <section className="key-policy-section">
          <h3>{t('keyAccess')}</h3>
          <fieldset className="key-policy-field">
            <legend>{t('routeGroups')}</legend>
            <div className="key-option-grid">
              {modelOptions.filter((model) => model.route).map((model) => (
                <label className={draft.models.includes(model.value) ? 'selected' : ''} key={model.value}>
                  <input type="checkbox" checked={draft.models.includes(model.value)} onChange={() => toggleListItem('models', model.value)} />
                  <span>{model.label}<small>{model.detail}</small></span>
                  {draft.models.includes(model.value) && <Check size={13} />}
                </label>
              ))}
            </div>
          </fieldset>
          <fieldset className="key-policy-field">
            <legend>{t('keyModels')}</legend>
            <div className="key-selection-actions">
              <button type="button" onClick={() => update('models', models)}>{t('selectAll')}</button>
              <button type="button" onClick={() => update('models', [])}>{t('clear')}</button>
              <span>{draft.models.length} / {models.length}</span>
            </div>
            <div className="key-option-grid">
              {modelOptions.filter((model) => !model.route).map((model) => (
                <label className={draft.models.includes(model.value) ? 'selected' : ''} key={model.value}>
                  <input type="checkbox" checked={draft.models.includes(model.value)} onChange={() => toggleListItem('models', model.value)} />
                  <span>{model.label}</span>
                  {draft.models.includes(model.value) && <Check size={13} />}
                </label>
              ))}
            </div>
          </fieldset>
          <fieldset className="key-policy-field">
            <legend>{t('keyGroups')}</legend>
            {channelGroups.length ? (
              <div className="key-option-grid">
                {channelGroups.map((group) => (
                  <label className={draft.allowedGroups.includes(group.id) ? 'selected' : ''} key={group.id}>
                    <input type="checkbox" checked={draft.allowedGroups.includes(group.id)} onChange={() => toggleListItem('allowedGroups', group.id)} />
                    <span>{groupLabel(group.id, group.name)}</span>
                    {draft.allowedGroups.includes(group.id) && <Check size={13} />}
                  </label>
                ))}
              </div>
            ) : <p className="key-empty-options">{t('keyNoGroups')}</p>}
          </fieldset>
          <fieldset className="key-policy-field">
            <legend>{t('keyExcluded')}</legend>
            <div className="key-option-grid">
              {providers.map((provider) => (
                <label className={draft.excludedProviders.includes(provider.name) ? 'selected' : ''} key={provider.id}>
                  <input type="checkbox" checked={draft.excludedProviders.includes(provider.name)} onChange={() => toggleListItem('excludedProviders', provider.name)} />
                  <span>{provider.name}</span>
                  {draft.excludedProviders.includes(provider.name) && <Check size={13} />}
                </label>
              ))}
            </div>
          </fieldset>
        </section>
        <div className="key-enabled-row"><span>{t('keyEnabled')}</span><Toggle label={t('keyEnabled')} checked={draft.enabled} onChange={() => update('enabled', !draft.enabled)} /></div>
        {error && <p className="key-policy-error" role="alert">{error}</p>}
      </div>
      <DialogActions onClose={close} label={t('generateKey')} />
    </form>
  </Modal>
}
