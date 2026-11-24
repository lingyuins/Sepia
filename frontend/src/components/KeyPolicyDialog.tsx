import { useState } from 'react'
import { Check, ChevronDown, ShieldCheck } from 'lucide-react'
import { parseKeyPolicy, type KeyPolicy, type KeyPolicyDraft } from '../key-policy'
import { scopedModels } from '../key-models'
import { useTranslation, useWorkspace } from '../workspace'
import { DialogActions, Modal, Toggle } from './UI'

export type PreviewKey = KeyPolicy & { id: number; value: string; created: string }
function KeyField({ label, value, onChange, placeholder = '', type = 'text', hint, required = false }: { label: string; value: string; onChange: (value: string) => void; placeholder?: string; type?: string; hint?: string; required?: boolean }) {
  return <label className="key-policy-field"><span>{label}</span><input className="field" type={type} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} required={required} min={type === 'number' ? '0' : undefined} step={type === 'number' ? 'any' : undefined} />{hint && <small>{hint}</small>}</label>
}

export default function KeyPolicyDialog({ open, onClose, onCreate }: { open: boolean; onClose: () => void; onCreate: (policy: KeyPolicy) => Promise<void> }) {
  const { providers, channelGroups, routes, modelGroups } = useWorkspace()
  const t = useTranslation()
  const modelOptions = scopedModels(providers, routes)
  const models = modelOptions.map((model) => model.value)
  const routeModels = modelOptions.filter((model) => model.route).map((model) => model.value)
  const [draft, setDraft] = useState<KeyPolicyDraft>(() => ({ name: '', customValue: '', maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', models, allowedGroups: [], modelGroups: [], excludedProviders: [], enabled: true }))
  const [limitsOpen, setLimitsOpen] = useState(true)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const update = <K extends keyof KeyPolicyDraft>(key: K, value: KeyPolicyDraft[K]) => { setDraft((current) => ({ ...current, [key]: value })); setError('') }
  const toggleListItem = (key: 'models' | 'allowedGroups' | 'excludedProviders', value: string) => update(key, draft[key].includes(value) ? draft[key].filter((item) => item !== value) : [...draft[key], value])
  const toggleModelGroup = (id: number) => update('modelGroups', draft.modelGroups.includes(id) ? draft.modelGroups.filter((item) => item !== id) : [...draft.modelGroups, id])
  // Members granted through a checked model group: shown checked but locked,
  // since the group (not the individual checkbox) authorizes them.
  const groupGranted = new Map<string, string>()
  for (const group of modelGroups) {
    if (!draft.modelGroups.includes(group.id)) continue
    for (const member of group.members) if (!groupGranted.has(member)) groupGranted.set(member, group.name)
  }
  const close = () => { setError(''); onClose() }
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (busy) return
    try {
      const policy = parseKeyPolicy(draft)
      if (!policy.name) { setError(t('keyPolicyName')); return }
      setBusy(true)
      try { await onCreate(policy); setDraft({ name: '', customValue: '', maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', models, allowedGroups: [], modelGroups: [], excludedProviders: [], enabled: true }) }
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
        </section>
        <section className={`key-policy-section key-limits-section ${limitsOpen ? 'expanded' : ''}`}>
          <h3><button type="button" className="key-section-toggle" aria-expanded={limitsOpen} onClick={() => setLimitsOpen(!limitsOpen)}><span>{t('keyLimits')}</span><ChevronDown size={15} /></button></h3>
          {limitsOpen && <>
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
          </>}
        </section>
        <section className="key-policy-section">
          <h3>{t('keyAccess')}</h3>
          <fieldset className="key-policy-field">
            <legend>{t('modelGroups')}</legend>
            {modelGroups.length ? (
              <div className="key-option-grid">
                {modelGroups.map((group) => (
                  <label className={draft.modelGroups.includes(group.id) ? 'selected' : ''} key={group.id}>
                    <input type="checkbox" checked={draft.modelGroups.includes(group.id)} onChange={() => toggleModelGroup(group.id)} />
                    <span>{group.name}<small>{group.members.length} {t('models')}</small></span>
                    {draft.modelGroups.includes(group.id) && <Check size={13} />}
                  </label>
                ))}
              </div>
            ) : <p className="key-empty-options">{t('modelGroupsEmpty')}</p>}
            <small>{t('modelGroupsHint')}</small>
          </fieldset>
          <fieldset className="key-policy-field">
            <legend>{t('routeGroups')}</legend>
            <div className="key-selection-actions">
              <button type="button" onClick={() => update('models', [...new Set([...draft.models, ...routeModels])])}>{t('selectAll')}</button>
              <button type="button" onClick={() => update('models', draft.models.filter((model) => !routeModels.includes(model)))}>{t('clear')}</button>
              <span>{routeModels.filter((model) => draft.models.includes(model) || groupGranted.has(model)).length} / {routeModels.length}</span>
            </div>
            <div className="key-option-grid">
              {modelOptions.filter((model) => model.route).map((model) => {
                const granted = groupGranted.get(model.value)
                const checked = draft.models.includes(model.value) || !!granted
                return (
                  <label className={`${checked ? 'selected' : ''} ${granted ? 'group-granted' : ''}`} key={model.value}>
                    <input type="checkbox" checked={checked} disabled={!!granted} onChange={() => toggleListItem('models', model.value)} />
                    <span>{model.label}<small>{granted ? t('modelGroupFromGroup', { name: granted }) : model.detail}</small></span>
                    {checked && <Check size={13} />}
                  </label>
                )
              })}
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
              {modelOptions.filter((model) => !model.route).map((model) => {
                const granted = groupGranted.get(model.value)
                const checked = draft.models.includes(model.value) || !!granted
                return (
                  <label className={`${checked ? 'selected' : ''} ${granted ? 'group-granted' : ''}`} key={model.value}>
                    <input type="checkbox" checked={checked} disabled={!!granted} onChange={() => toggleListItem('models', model.value)} />
                    <span>{model.label}{granted && <small>{t('modelGroupFromGroup', { name: granted })}</small>}</span>
                    {checked && <Check size={13} />}
                  </label>
                )
              })}
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
