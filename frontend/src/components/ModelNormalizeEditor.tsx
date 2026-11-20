import { useRef, useState } from 'react'
import { AlertCircle, ArrowRight, Check, Copy, Download, Plus, RotateCcw, Upload, X } from 'lucide-react'
import { downloadFile, useTranslation, useWorkspace } from '../workspace'
import { BUILTIN_FUNCTIONAL_SUFFIXES, BUILTIN_ROUTER_PREFIXES, compileNormalizeRules, normalizeModelName, validateNormalizeRules, type ExplicitMapping, type RuleError } from '../model-normalize'
import { buildModelNamesExport, candidateCoverage, modelChannelsFilename, parseRulesFile, type WorkflowRules } from '../normalize-workflow'
import type { TranslationKey } from '../i18n'

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
  const { providers, notify } = useWorkspace()
  const fileInput = useRef<HTMLInputElement>(null)
  const [copied, setCopied] = useState(false)
  const [imported, setImported] = useState<{ rules: WorkflowRules; error: RuleError | null } | null>(null)
  const names = providers.flatMap((provider) => provider.modelsList)
  const copyPrompt = async () => {
    try {
      await navigator.clipboard.writeText(t('normalizeWorkflowPrompt'))
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch { notify(t('normalizePromptCopyFailed')) }
  }
  const exportNames = () => {
    downloadFile(modelChannelsFilename(), JSON.stringify(buildModelNamesExport(names), null, 2))
  }
  const importFile = async (file: File) => {
    // Drop any previous preview first: a failed import must never leave a
    // stale rule set that Apply could still write.
    setImported(null)
    const result = parseRulesFile(await file.text())
    if (!result.ok) {
      notify(t(result.error === 'shape' ? 'normalizeImportShape' : result.error === 'arrays' ? 'normalizeImportArrays' : 'normalizeImportEntries'))
      return
    }
    setImported({ rules: result.rules, error: validateNormalizeRules(result.rules) })
  }
  const applyImport = () => {
    if (!imported || imported.error) return
    onChange({ prefixes: imported.rules.routerPrefixes, suffixes: imported.rules.functionalSuffixes, mappings: imported.rules.explicitMappings })
    setImported(null)
    notify(t('normalizeImportApplied'))
  }
  const importedCoverage = imported ? candidateCoverage(names, (name) => normalizeModelName(name, compileNormalizeRules(imported.rules))) : null
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
    <div className="normalize-workflow">
      <div className="settings-card-title"><div><h3>{t('normalizeWorkflowTitle')}</h3><p>{t('normalizeWorkflowDescription')}</p></div></div>
      <div className="normalize-workflow-steps">
      <fieldset className="settings-field normalize-rule-field"><legend>{t('normalizePromptCopy')}<span className="normalize-legend-info" title={t('normalizePromptHint')}><AlertCircle size={12} aria-label={t('normalizePromptHint')} /></span></legend>
        <div className="key-tag-input">
          <button type="button" className="button secondary" onClick={() => void copyPrompt()}>{copied ? <Check size={13} /> : <Copy size={13} />}{copied ? t('normalizePromptCopied') : t('normalizePromptCopy')}</button>
        </div>
      </fieldset>
      <fieldset className="settings-field normalize-rule-field"><legend>{t('normalizeExportNames')}<span className="normalize-legend-info" title={t('normalizeExportHint')}><AlertCircle size={12} aria-label={t('normalizeExportHint')} /></span></legend>
        <div className="key-tag-input">
          <button type="button" className="button secondary" disabled={names.length === 0} onClick={exportNames}><Download size={13} />{t('normalizeExportNames')}</button>
          <span className="muted">{names.length === 0 ? t('normalizeExportEmpty') : t('normalizeExportCount', { count: names.length })}</span>
        </div>
      </fieldset>
      <fieldset className="settings-field normalize-rule-field"><legend>{t('normalizeImportRules')}<span className="normalize-legend-info" title={t('normalizeImportHint')}><AlertCircle size={12} aria-label={t('normalizeImportHint')} /></span></legend>
        <div className="key-tag-input">
          <button type="button" className="button secondary" onClick={() => fileInput.current?.click()}><Upload size={13} />{t('normalizeImportRules')}</button>
          <input ref={fileInput} type="file" accept="application/json,.json" hidden aria-label={t('normalizeImportRules')} onChange={(event) => { const file = event.target.files?.[0]; if (file) void importFile(file); event.target.value = '' }} />
        </div>
        {imported && <div className="normalize-import-preview">
          <strong>{t('normalizeImportPreview')}</strong>
          <div className="key-tag-list">
            <span className="key-tag">{t('normalizePrefixes')}: {imported.rules.routerPrefixes.length}</span>
            <span className="key-tag">{t('normalizeSuffixes')}: {imported.rules.functionalSuffixes.length}</span>
            <span className="key-tag">{t('normalizeMappings')}: {imported.rules.explicitMappings.length}</span>
          </div>
          {importedCoverage && <div className="key-tag-list">
            <span className="key-tag">{t('normalizeCoverageInput')}: {importedCoverage.input}</span>
            <span className="key-tag">{t('normalizeCoverageCanonical')}: {importedCoverage.canonical}</span>
            <span className="key-tag">{t('normalizeCoverageMerged')}: {importedCoverage.merged}</span>
          </div>}
          {imported.error && <p className="danger-text normalize-import-error">{t(imported.error.key as TranslationKey, imported.error.params)}</p>}
          <div className="key-tag-input">
            <button type="button" className="button" disabled={!!imported.error} onClick={applyImport}>{t('normalizeImportApply')}</button>
            <button type="button" className="button secondary" onClick={() => setImported(null)}>{t('cancel')}</button>
          </div>
        </div>}
      </fieldset>
      </div>
    </div>
  </section>
}