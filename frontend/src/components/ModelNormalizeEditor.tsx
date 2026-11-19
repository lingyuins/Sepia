import { useState } from 'react'
import { ArrowRight, Plus, RotateCcw, X } from 'lucide-react'
import { useTranslation } from '../workspace'
import { BUILTIN_FUNCTIONAL_SUFFIXES, BUILTIN_ROUTER_PREFIXES, compileNormalizeRules, normalizeModelName, type ExplicitMapping } from '../model-normalize'

export type NormalizeRulesDraft = { prefixes: string[]; suffixes: string[]; mappings: ExplicitMapping[] }

function RuleList({ label, hint, placeholder, values, builtin, onChange }: { label: string; hint: string; placeholder: string; values: string[]; builtin: string[]; onChange: (values: string[]) => void }) {
  const t = useTranslation()
  const [input, setInput] = useState('')
  const add = () => {
    const value = input.trim()
    if (value && !values.includes(value)) onChange([...values, value])
    setInput('')
  }
  return <fieldset className="settings-field normalize-rule-field"><legend>{label}</legend>
    <div className="key-tag-list">
      {values.length === 0 && builtin.map((value) => <span className="key-tag normalize-builtin-tag" key={value} title={t('normalizeBuiltinHint')}>{value}</span>)}
      {values.map((value) => <span className="key-tag" key={value}>{value}<button type="button" onClick={() => onChange(values.filter((item) => item !== value))} aria-label={t('deleteNamed', { name: value })}><X size={11} /></button></span>)}
    </div>
    <div className="key-tag-input">
      <input className="field" value={input} aria-label={label} placeholder={placeholder} onChange={(event) => setInput(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); add() } }} />
      <button type="button" className="button secondary" onClick={add}><Plus size={13} />{t('normalizeAddRule')}</button>
      <button type="button" className="button secondary" disabled={values.length === 0} onClick={() => onChange([])} title={t('normalizeBuiltinHint')}><RotateCcw size={13} />{t('reset')}</button>
    </div>
    <small>{values.length === 0 ? t('normalizeBuiltinHint') : hint}</small>
  </fieldset>
}

export default function ModelNormalizeEditor({ prefixes, suffixes, mappings, onChange }: NormalizeRulesDraft & { onChange: (next: NormalizeRulesDraft) => void }) {
  const t = useTranslation()
  const [variant, setVariant] = useState('')
  const [canonical, setCanonical] = useState('')
  const [preview, setPreview] = useState('')
  const rules = compileNormalizeRules({ routerPrefixes: prefixes, functionalSuffixes: suffixes, explicitMappings: mappings })
  const addMapping = () => {
    const from = variant.trim()
    const to = canonical.trim().toLowerCase()
    if (!from || !to || mappings.some((mapping) => mapping.variant === from)) return
    onChange({ prefixes, suffixes, mappings: [...mappings, { variant: from, canonical: to }] })
    setVariant('')
    setCanonical('')
  }
  return <section className="settings-card glass-panel model-normalize-card">
    <div className="settings-card-title"><div><h3>{t('normalizeTitle')}</h3><p>{t('normalizeDescription')}</p></div></div>
    <div className="settings-fields">
      <RuleList label={t('normalizePrefixes')} hint={t('normalizePrefixesHint')} placeholder={t('normalizePrefixPlaceholder')} values={prefixes} builtin={BUILTIN_ROUTER_PREFIXES} onChange={(next) => onChange({ prefixes: next, suffixes, mappings })} />
      <RuleList label={t('normalizeSuffixes')} hint={t('normalizeSuffixesHint')} placeholder={t('normalizeSuffixPlaceholder')} values={suffixes} builtin={BUILTIN_FUNCTIONAL_SUFFIXES} onChange={(next) => onChange({ prefixes, suffixes: next, mappings })} />
    </div>
    <fieldset className="settings-field normalize-rule-field"><legend>{t('normalizeMappings')}</legend>
      {mappings.length > 0 && <div className="normalize-mapping-list">{mappings.map((mapping, index) => <div className="normalize-mapping-row" key={mapping.variant}>
        <code>{mapping.variant}</code><ArrowRight size={13} /><code>{mapping.canonical}</code>
        <button type="button" className="icon-button" aria-label={t('deleteNamed', { name: mapping.variant })} onClick={() => onChange({ prefixes, suffixes, mappings: mappings.filter((_, i) => i !== index) })}><X size={13} /></button>
      </div>)}</div>}
      <div className="normalize-mapping-input">
        <input className="field" value={variant} aria-label={t('normalizeVariant')} placeholder={t('normalizeVariantPlaceholder')} onChange={(event) => setVariant(event.target.value)} />
        <ArrowRight size={14} />
        <input className="field" value={canonical} aria-label={t('normalizeCanonical')} placeholder={t('normalizeCanonicalPlaceholder')} onChange={(event) => setCanonical(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addMapping() } }} />
        <button type="button" className="button secondary" onClick={addMapping}><Plus size={13} />{t('normalizeAddMapping')}</button>
      </div>
      <small>{t('normalizeMappingsHint')}</small>
    </fieldset>
    <label className="settings-field normalize-preview"><span>{t('normalizePreview')}</span>
      <div className="normalize-preview-row">
        <input className="field" value={preview} aria-label={t('normalizePreview')} placeholder="dmxapi-kimi-k2.5-cc" onChange={(event) => setPreview(event.target.value)} />
        <ArrowRight size={14} />
        <code>{preview.trim() ? normalizeModelName(preview, rules) : '—'}</code>
      </div>
      <small>{t('normalizePreviewHint')}</small>
    </label>
  </section>
}