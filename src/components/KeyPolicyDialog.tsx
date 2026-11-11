import { useState } from 'react'
import { Check, Plus, ShieldCheck, X } from 'lucide-react'
import { parseKeyPolicy, type KeyPolicy, type KeyPolicyDraft } from '../key-policy'
import { useTranslation, useWorkspace } from '../workspace'
import { DialogActions, Modal, Toggle } from './UI'

export type PreviewKey = KeyPolicy & { id: number; value: string; created: string }
type KeyDialogCopy = {
  identity: string; limits: string; access: string; name: string; namePlaceholder: string; customKey: string; generate: string; tags: string; addTag: string; tagPlaceholder: string;
  maxAmount: string; amountPlaceholder: string; unlimited: string; tokenLimit: string; tokenPlaceholder: string; rpm: string; tpm: string; quotas: string; ipWhitelist: string; ipPlaceholder: string;
  expiry: string; never: string; models: string; groups: string; noGroups: string; excluded: string; enabled: string; selectAll: string; clear: string; error: string;
}
const copy: Record<string, KeyDialogCopy> = {
  English: {
    identity: 'Basic information', limits: 'Usage & rate limits', access: 'Access scope', name: 'Name', namePlaceholder: 'bot', customKey: 'Custom key', generate: 'Generate', tags: 'Tags', addTag: 'Add', tagPlaceholder: 'Client, Code…',
    maxAmount: 'Maximum amount ($)', amountPlaceholder: 'Enter amount', unlimited: 'Unlimited when empty', tokenLimit: 'Token limit', tokenPlaceholder: 'e.g. 100w, 5亿, 1000000', rpm: 'Rate limit (RPM)', tpm: 'Rate limit (TPM)', quotas: 'Per-model quotas (JSON)', ipWhitelist: 'IP whitelist', ipPlaceholder: 'Comma-separated IP or CIDR, e.g. 10.0.0.1,192.168.0.0/16',
    expiry: 'Expiration date', never: 'Never expires', models: 'Supported models', groups: 'Allowed groups', noGroups: 'No groups available', excluded: 'Excluded providers', enabled: 'Enable this key', selectAll: 'Select all', clear: 'Clear', error: 'Check the limits, quota JSON, IP addresses, and custom key.',
  },
  简体中文: {
    identity: '基本信息', limits: '用量与速率限制', access: '访问范围', name: '名称', namePlaceholder: 'bot', customKey: '自定义密钥', generate: '自动生成', tags: '标签', addTag: '添加', tagPlaceholder: '客户端、Code…',
    maxAmount: '最大金额（$）', amountPlaceholder: '请输入金额', unlimited: '留空表示无限制', tokenLimit: 'Token 上限', tokenPlaceholder: '如 100w、5亿、1000000', rpm: '速率限制（RPM）', tpm: '速率限制（TPM）', quotas: '按模型配额（JSON）', ipWhitelist: 'IP 白名单', ipPlaceholder: '逗号分隔的 IP 或 CIDR，如 10.0.0.1,192.168.0.0/16',
    expiry: '过期日期', never: '永不过期', models: '支持的模型', groups: '允许的分组分类', noGroups: '暂无分组分类', excluded: '排除渠道', enabled: '是否启用', selectAll: '全选', clear: '清空', error: '请检查额度、模型配额 JSON、IP 地址和自定义密钥。',
  },
  日本語: {
    identity: '基本情報', limits: '使用量とレート制限', access: 'アクセス範囲', name: '名前', namePlaceholder: 'bot', customKey: 'カスタムキー', generate: '生成', tags: 'タグ', addTag: '追加', tagPlaceholder: 'クライアント、Code…',
    maxAmount: '上限金額（$）', amountPlaceholder: '金額を入力', unlimited: '空欄で無制限', tokenLimit: 'トークン上限', tokenPlaceholder: '例: 100w、5億、1000000', rpm: 'レート制限（RPM）', tpm: 'レート制限（TPM）', quotas: 'モデル別クォータ（JSON）', ipWhitelist: 'IP ホワイトリスト', ipPlaceholder: 'IP または CIDR をカンマ区切りで入力',
    expiry: '有効期限', never: '無期限', models: '対応モデル', groups: '許可するグループ', noGroups: 'グループはありません', excluded: '除外プロバイダー', enabled: 'このキーを有効化', selectAll: 'すべて選択', clear: 'クリア', error: '上限、クォータ JSON、IP アドレス、カスタムキーを確認してください。',
  },
}
function KeyField({ label, value, onChange, placeholder = '', type = 'text', hint, required = false }: { label: string; value: string; onChange: (value: string) => void; placeholder?: string; type?: string; hint?: string; required?: boolean }) {
  return <label className="key-policy-field"><span>{label}</span><input className="field" type={type} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} required={required} min={type === 'number' ? '0' : undefined} step={type === 'number' ? 'any' : undefined} />{hint && <small>{hint}</small>}</label>
}

export default function KeyPolicyDialog({ open, onClose, onCreate }: { open: boolean; onClose: () => void; onCreate: (policy: KeyPolicy) => Promise<void> }) {
  const { settings, providers, channelGroups } = useWorkspace()
  const t = useTranslation()
  const c = copy[settings.language] || copy.English
  const models = [...new Set(providers.flatMap((provider) => provider.modelsList))]
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
      if (!policy.name) { setError(c.name); return }
      setBusy(true)
      try { await onCreate(policy); setDraft({ name: '', customValue: '', tags: [], maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', models, allowedGroups: [], excludedProviders: [], enabled: true }) }
      catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
      finally { setBusy(false) }
    } catch { setError(c.error) }
  }
  const groupLabel = (id: string, fallback: string) => id === 'official' ? t('officialChannels') : id === 'free' ? t('freeChannels') : id === 'team' ? t('teamChannels') : fallback

  return <Modal open={open} onClose={close} title={t('newKeyTitle')} description={t('newKeyDescription')} size="key-policy-dialog">
    <form className="key-policy-form" onSubmit={submit}>
      <div className="key-policy-scroll">
        <section className="key-policy-section"><h3>{c.identity}</h3><div className="key-policy-grid"><KeyField label={c.name} value={draft.name} onChange={(value) => update('name', value)} placeholder={c.namePlaceholder} required /></div>
          <fieldset className="key-policy-field key-tags-field"><legend>{c.tags}</legend><div className="key-tag-list">{draft.tags.map((tag) => <span className="key-tag" key={tag}>{tag}<button type="button" onClick={() => update('tags', draft.tags.filter((item) => item !== tag))} aria-label={`${c.tags}: ${tag}`}><X size={11} /></button></span>)}</div><div className="key-tag-input"><input className="field" value={tagDraft} onChange={(event) => setTagDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addTag(tagDraft) } }} placeholder={c.tagPlaceholder} /><button type="button" className="button secondary" onClick={() => addTag(tagDraft)}><Plus size={13} />{c.addTag}</button></div></fieldset>
        </section>
        <section className="key-policy-section"><h3>{c.limits}</h3><div className="key-policy-grid"><KeyField label={c.maxAmount} value={draft.maxAmount} onChange={(value) => update('maxAmount', value)} placeholder={c.amountPlaceholder} type="number" hint={c.unlimited} /><KeyField label={c.tokenLimit} value={draft.tokenLimit} onChange={(value) => update('tokenLimit', value)} placeholder={c.tokenPlaceholder} hint={c.unlimited} /><KeyField label={c.rpm} value={draft.rpm} onChange={(value) => update('rpm', value)} type="number" /><KeyField label={c.tpm} value={draft.tpm} onChange={(value) => update('tpm', value)} type="number" /></div><label className="key-policy-field"><span>{c.quotas}</span><textarea className="field key-quota-input" value={draft.modelQuotas} placeholder={'{"gpt-4o":{"rpm":5,"tpm":50000}}'} onChange={(event) => update('modelQuotas', event.target.value)} spellCheck={false} rows={3} /></label><KeyField label={c.ipWhitelist} value={draft.ipWhitelist} onChange={(value) => update('ipWhitelist', value)} placeholder={c.ipPlaceholder} /><div className="key-policy-grid"><KeyField label={c.expiry} value={draft.expiresAt} onChange={(value) => update('expiresAt', value)} type="datetime-local" /><button type="button" className="key-never-expiry" onClick={() => update('expiresAt', '')}><ShieldCheck size={15} />{c.never}{!draft.expiresAt && <Check size={14} />}</button></div></section>
        <section className="key-policy-section"><h3>{c.access}</h3><fieldset className="key-policy-field"><legend>{c.models}</legend><div className="key-selection-actions"><button type="button" onClick={() => update('models', models)}>{c.selectAll}</button><button type="button" onClick={() => update('models', [])}>{c.clear}</button><span>{draft.models.length} / {models.length}</span></div><div className="key-option-grid">{models.map((model) => <label className={draft.models.includes(model) ? 'selected' : ''} key={model}><input type="checkbox" checked={draft.models.includes(model)} onChange={() => toggleListItem('models', model)} /><span>{model}</span>{draft.models.includes(model) && <Check size={13} />}</label>)}</div></fieldset>
          <fieldset className="key-policy-field"><legend>{c.groups}</legend>{channelGroups.length ? <div className="key-option-grid">{channelGroups.map((group) => <label className={draft.allowedGroups.includes(group.id) ? 'selected' : ''} key={group.id}><input type="checkbox" checked={draft.allowedGroups.includes(group.id)} onChange={() => toggleListItem('allowedGroups', group.id)} /><span>{groupLabel(group.id, group.name)}</span>{draft.allowedGroups.includes(group.id) && <Check size={13} />}</label>)}</div> : <p className="key-empty-options">{c.noGroups}</p>}</fieldset>
          <fieldset className="key-policy-field"><legend>{c.excluded}</legend><div className="key-option-grid">{providers.map((provider) => <label className={draft.excludedProviders.includes(provider.name) ? 'selected' : ''} key={provider.id}><input type="checkbox" checked={draft.excludedProviders.includes(provider.name)} onChange={() => toggleListItem('excludedProviders', provider.name)} /><span>{provider.name}</span>{draft.excludedProviders.includes(provider.name) && <Check size={13} />}</label>)}</div></fieldset>
        </section>
        <div className="key-enabled-row"><span>{c.enabled}</span><Toggle label={c.enabled} checked={draft.enabled} onChange={() => update('enabled', !draft.enabled)} /></div>
        {error && <p className="key-policy-error" role="alert">{error}</p>}
      </div>
      <DialogActions onClose={close} label={t('generateKey')} />
    </form>
  </Modal>
}
