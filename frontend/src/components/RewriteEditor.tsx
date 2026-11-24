import { useState } from 'react'
import { ChevronDown, Plus, X } from 'lucide-react'
import { useTranslation } from '../workspace'
import {
  formatParamValue,
  isReservedHeader,
  parseParamValue,
  rewriteCount,
  testRegex,
  validateRewrite,
  type ParamDefaultRule,
  type ParamReplaceRule,
  type PromptReplaceRule,
  type RewriteConfig,
} from '../rewrite'

// Shared editor for provider- and route-level request rewrites. The dialog
// owns the value; every row edit publishes a complete new RewriteConfig.
export function RewriteEditor({
  value,
  onChange,
}: {
  value: RewriteConfig
  onChange: (next: RewriteConfig) => void
}) {
  const t = useTranslation()
  const [open, setOpen] = useState(false)
  const [paramSamples, setParamSamples] = useState<string[]>(() => value.paramReplace.map(() => ''))
  const [promptSamples, setPromptSamples] = useState<string[]>(() => value.promptReplace.map(() => ''))
  const count = rewriteCount(value)
  const issues = validateRewrite(value)

  const setParamReplace = (paramReplace: ParamReplaceRule[]) => onChange({ ...value, paramReplace })
  const updateParam = (index: number, patch: Partial<ParamReplaceRule>) =>
    setParamReplace(value.paramReplace.map((rule, i) => (i === index ? { ...rule, ...patch } : rule)))
  const addParam = () => {
    setParamSamples((current) => [...current, ''])
    setParamReplace([...value.paramReplace, { field: '', pattern: '', replacement: '' }])
  }
  const removeParam = (index: number) => {
    setParamSamples((current) => current.filter((_, i) => i !== index))
    setParamReplace(value.paramReplace.filter((_, i) => i !== index))
  }

  const setPromptReplace = (promptReplace: PromptReplaceRule[]) => onChange({ ...value, promptReplace })
  const addPrompt = () => {
    setPromptSamples((current) => [...current, ''])
    setPromptReplace([...value.promptReplace, { pattern: '', replacement: '' }])
  }
  const removePrompt = (index: number) => {
    setPromptSamples((current) => current.filter((_, i) => i !== index))
    setPromptReplace(value.promptReplace.filter((_, i) => i !== index))
  }

  const setParamDefaults = (paramDefaults: ParamDefaultRule[]) => onChange({ ...value, paramDefaults })
  const updateDefaultValue = (index: number, text: string) => {
    setParamDefaults(
      value.paramDefaults.map((rule, i) =>
        i === index ? { ...rule, text, value: parseParamValue(text) } : rule,
      ),
    )
  }
  const addDefault = () => {
    setParamDefaults([...value.paramDefaults, { field: '', value: undefined, text: '' }])
  }
  const removeDefault = (index: number) => {
    setParamDefaults(value.paramDefaults.filter((_, i) => i !== index))
  }

  const preview = (pattern: string, replacement: string, sample: string) => {
    if (!pattern.trim() || !sample) return ''
    try {
      return testRegex(pattern, replacement, sample)
    } catch {
      return ''
    }
  }

  return (
    <section className={`rewrite-section ${open ? 'expanded' : ''}`}>
      <h3>
        <button
          type="button"
          className="key-section-toggle"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <span>{t('rewriteSection')}</span>
          {count > 0 && <small className="rewrite-count tabular">{count}</small>}
          <ChevronDown size={15} />
        </button>
      </h3>
      {open && (
        <div className="rewrite-body">
          <p className="rewrite-hint">{t('rewriteSectionHint')}</p>

          <div className="rewrite-block">
            <div className="rewrite-block-head">
              <strong>{t('rewriteHeaders')}</strong>
              <button type="button" className="button text-button" onClick={() => onChange({ ...value, headers: [...value.headers, { name: '', value: '' }] })}>
                <Plus size={12} />
                {t('rewriteAddRow')}
              </button>
            </div>
            <p className="rewrite-hint">{t('rewriteHeadersHint')}</p>
            <div className="rewrite-rows">
              {value.headers.map((rule, index) => (
                <div className="rewrite-row" key={index}>
                  <input
                    className="field"
                    value={rule.name}
                    placeholder="X-Header"
                    aria-label={t('rewriteHeaderName')}
                    aria-invalid={isReservedHeader(rule.name) || undefined}
                    onChange={(event) => onChange({ ...value, headers: value.headers.map((rule, i) => (i === index ? { ...rule, name: event.target.value } : rule)) })}
                  />
                  <input
                    className="field"
                    value={rule.value}
                    placeholder={t('rewriteHeaderValue')}
                    aria-label={t('rewriteHeaderValue')}
                    onChange={(event) => onChange({ ...value, headers: value.headers.map((rule, i) => (i === index ? { ...rule, value: event.target.value } : rule)) })}
                  />
                  <button
                    type="button"
                    className="icon-button"
                    onClick={() => onChange({ ...value, headers: value.headers.filter((_, i) => i !== index) })}
                    aria-label={t('rewriteRemoveRow')}
                  >
                    <X size={14} />
                  </button>
                </div>
              ))}
            </div>
          </div>

          <div className="rewrite-block">
            <div className="rewrite-block-head">
              <strong>{t('rewriteParamReplace')}</strong>
              <button type="button" className="button text-button" onClick={addParam}>
                <Plus size={12} />
                {t('rewriteAddRow')}
              </button>
            </div>
            <p className="rewrite-hint">{t('rewriteParamReplaceHint')}</p>
            <div className="rewrite-rows">
              {value.paramReplace.map((rule, index) => (
                <div className="rewrite-rule" key={index}>
                  <div className="rewrite-row rewrite-row-3">
                    <input
                      className="field"
                      value={rule.field}
                      placeholder={t('rewriteField')}
                      aria-label={t('rewriteField')}
                      onChange={(event) => updateParam(index, { field: event.target.value })}
                    />
                    <input
                      className="field mono"
                      value={rule.pattern}
                      placeholder={t('rewritePattern')}
                      aria-label={t('rewritePattern')}
                      onChange={(event) => updateParam(index, { pattern: event.target.value })}
                    />
                    <input
                      className="field mono"
                      value={rule.replacement}
                      placeholder={t('rewriteReplacement')}
                      aria-label={t('rewriteReplacement')}
                      onChange={(event) => updateParam(index, { replacement: event.target.value })}
                    />
                    <button
                      type="button"
                      className="icon-button"
                      onClick={() => removeParam(index)}
                      aria-label={t('rewriteRemoveRow')}
                    >
                      <X size={14} />
                    </button>
                  </div>
                  <div className="rewrite-preview-row">
                    <input
                      className="field"
                      value={paramSamples[index] ?? ''}
                      placeholder={t('rewriteTestSample')}
                      aria-label={t('rewriteTestSample')}
                      onChange={(event) =>
                        setParamSamples((current) =>
                          value.paramReplace.map((_, i) =>
                            i === index ? event.target.value : current[i] ?? '',
                          ),
                        )
                      }
                    />
                    <code className="rewrite-preview">
                      {preview(rule.pattern, rule.replacement, paramSamples[index] ?? '')}
                    </code>
                  </div>
                </div>
              ))}
            </div>
            <small className="rewrite-hint">{t('rewriteModelHint')}</small>
          </div>

          <div className="rewrite-block">
            <div className="rewrite-block-head">
              <strong>{t('rewriteParamDefaults')}</strong>
              <button type="button" className="button text-button" onClick={addDefault}>
                <Plus size={12} />
                {t('rewriteAddRow')}
              </button>
            </div>
            <p className="rewrite-hint">{t('rewriteParamDefaultsHint')}</p>
            <div className="rewrite-rows">
              {value.paramDefaults.map((rule, index) => (
                <div className="rewrite-row" key={index}>
                  <input
                    className="field"
                    value={rule.field}
                    placeholder={t('rewriteField')}
                    aria-label={t('rewriteField')}
                    onChange={(event) => setParamDefaults(value.paramDefaults.map((rule, i) => (i === index ? { ...rule, field: event.target.value } : rule)))}
                  />
                  <input
                    className="field mono"
                    value={rule.text ?? formatParamValue(rule.value)}
                    placeholder={t('rewriteDefaultValue')}
                    aria-label={t('rewriteDefaultValue')}
                    aria-invalid={rule.value === undefined || undefined}
                    onChange={(event) => updateDefaultValue(index, event.target.value)}
                  />
                  <button
                    type="button"
                    className="icon-button"
                    onClick={() => removeDefault(index)}
                    aria-label={t('rewriteRemoveRow')}
                  >
                    <X size={14} />
                  </button>
                </div>
              ))}
            </div>
          </div>

          <div className="rewrite-block">
            <div className="rewrite-block-head">
              <strong>{t('rewritePromptReplace')}</strong>
              <button type="button" className="button text-button" onClick={addPrompt}>
                <Plus size={12} />
                {t('rewriteAddRow')}
              </button>
            </div>
            <p className="rewrite-hint">{t('rewritePromptReplaceHint')}</p>
            <p className="rewrite-warning">{t('rewritePromptSystemOnly')}</p>
            <div className="rewrite-rows">
              {value.promptReplace.map((rule, index) => (
                <div className="rewrite-rule" key={index}>
                  <div className="rewrite-row">
                    <input
                      className="field mono"
                      value={rule.pattern}
                      placeholder={t('rewritePattern')}
                      aria-label={t('rewritePattern')}
                      onChange={(event) => setPromptReplace(value.promptReplace.map((rule, i) => (i === index ? { ...rule, pattern: event.target.value } : rule)))}
                    />
                    <input
                      className="field mono"
                      value={rule.replacement}
                      placeholder={t('rewriteReplacement')}
                      aria-label={t('rewriteReplacement')}
                      onChange={(event) => setPromptReplace(value.promptReplace.map((rule, i) => (i === index ? { ...rule, replacement: event.target.value } : rule)))}
                    />
                    <button
                      type="button"
                      className="icon-button"
                      onClick={() => removePrompt(index)}
                      aria-label={t('rewriteRemoveRow')}
                    >
                      <X size={14} />
                    </button>
                  </div>
                  <div className="rewrite-preview-row">
                    <input
                      className="field"
                      value={promptSamples[index] ?? ''}
                      placeholder={t('rewriteTestSample')}
                      aria-label={t('rewriteTestSample')}
                      onChange={(event) =>
                        setPromptSamples((current) =>
                          value.promptReplace.map((_, i) =>
                            i === index ? event.target.value : current[i] ?? '',
                          ),
                        )
                      }
                    />
                    <code className="rewrite-preview">
                      {preview(rule.pattern, rule.replacement, promptSamples[index] ?? '')}
                    </code>
                  </div>
                </div>
              ))}
            </div>
            <small className="rewrite-hint">{t('rewriteRegexCompat')}</small>
          </div>

          {!!issues.length && (
            <ul className="rewrite-issues" role="alert">
              {issues.map((issue, index) => (
                <li key={index}>{t(issue.key, issue.values)}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  )
}
