import { useState } from 'react'
import { Check, RefreshCw, Sparkles, X } from 'lucide-react'
import { api, routeInput } from '../api'
import { useWorkspace, useTranslation } from '../workspace'
import { Field, Modal } from './UI'
import { filterGeneratedRoutes, generatedRouteRule, type GenerateResponse } from '../ai-pool'

type GeneratedState = { routes: GenerateResponse['routes']; errors: string[]; batches: number; services: number }

export function AiRouteDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { providers, routes, settings, run, refresh, notify } = useWorkspace()
  const t = useTranslation()
  const [instruction, setInstruction] = useState('')
  const [batchSize, setBatchSize] = useState('10')
  const [running, setRunning] = useState(false)
  const [error, setError] = useState('')
  const [result, setResult] = useState<GeneratedState | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [applying, setApplying] = useState(false)
  const visible = result ? filterGeneratedRoutes(result.routes, providers, routes.map((route) => route.slug || '')) : []
  const generate = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!instruction.trim()) { notify(t('aiGenerateInstructionRequired')); return }
    const size = Number(batchSize)
    if (!Number.isInteger(size) || size < 1 || size > 50) { notify(t('aiGenerateBatchInvalid')); return }
    setRunning(true); setError(''); setResult(null); setSelected(new Set())
    try {
      const response = await api<GenerateResponse>('/api/v1/ai/routes/generate', { method: 'POST', body: JSON.stringify({ instruction: instruction.trim(), batch_size: size }) })
      setResult({ routes: response.routes, errors: response.errors, batches: response.batches, services: response.services })
      setSelected(new Set(filterGeneratedRoutes(response.routes, providers, routes.map((route) => route.slug || '')).map((route) => route.slug)))
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setRunning(false) }
  }
  const toggle = (slug: string) => setSelected((current) => { const next = new Set(current); if (next.has(slug)) next.delete(slug); else next.add(slug); return next })
  const apply = async () => {
    const chosen = visible.filter((route) => selected.has(route.slug))
    if (!chosen.length) return
    setApplying(true)
    const created: string[] = []
    try {
      await run(async () => {
        for (const [index, route] of chosen.entries()) {
          await api('/api/v1/routes', { method: 'POST', body: JSON.stringify(routeInput(generatedRouteRule(route, index), providers)) })
          created.push(route.slug)
        }
      }, ['routes'])
      notify(t('aiRoutesApplied', { count: chosen.length }))
      onClose()
    } catch (cause) {
      // Partial apply: refresh so the created routes are visible, then drop the
      // already-created proposals so a retry only sends the remainder instead
      // of re-creating old slugs.
      if (created.length) {
        setResult((current) => current && { ...current, routes: current.routes.filter((route) => !created.includes(route.slug)) })
        setSelected((current) => new Set([...current].filter((slug) => !created.includes(slug))))
      }
      try {
        await refresh(undefined, ['routes'])
      } catch {}
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally { setApplying(false) }
  }
  return <Modal open={open} onClose={onClose} title={t('aiGenerateTitle')} description={t('aiGenerateSubtitle')} size="route-dialog">
    <form className="ai-generate-form" onSubmit={generate}>
      <label className="form-field"><span className="field-label">{t('aiGenerateInstruction')}</span><textarea className="field ai-generate-instruction" rows={3} value={instruction} onChange={(event) => setInstruction(event.target.value)} placeholder={t('aiGenerateInstructionPlaceholder')} /></label>
      <div className="ai-generate-controls">
        <Field label={t('aiGenerateBatchSize')} value={batchSize} onChange={setBatchSize} type="number" hint={t('aiGenerateBatchHint')} />
        <button type="submit" className="button primary" disabled={running || !settings.aiPoolEnabled}>{running ? <RefreshCw size={14} className="spin" /> : <Sparkles size={14} />}{running ? t('aiGenerateRunning') : t('aiGenerateAction')}</button>
      </div>
    </form>
    {!settings.aiPoolEnabled && <p className="route-test-error">{t('aiPoolDisabledHint')}</p>}
    {error && <p className="route-test-error" role="alert">{error}</p>}
    {result && <div className="route-test-result ai-generate-result">
      <p className="route-section-hint">{t('aiGenerateSummary', { batches: result.batches, services: result.services, count: visible.length })}</p>
      {result.errors.map((message, index) => <p className="route-test-error" key={index}>{message}</p>)}
      {visible.map((route) => <label className="test-step ai-route-proposal" key={route.slug}>
        <input type="checkbox" checked={selected.has(route.slug)} onChange={() => toggle(route.slug)} aria-label={route.name || route.slug} />
        <div><strong>{route.name || route.slug}</strong>
          <p>{route.candidates.map((candidate) => `${candidate.provider} · ${candidate.model}`).join(' → ')}</p>
          <small>{t('whenModelMatches')}: {route.patterns.join(' · ')}</small>
        </div>
      </label>)}
      {visible.length === 0 && <div className="empty-state">{t('aiGenerateNoRoutes')}</div>}
      {visible.length > 0 && <div className="route-test-actions">
        <button type="button" className="button primary" disabled={applying || selected.size === 0} onClick={() => void apply()}>{applying ? <RefreshCw size={14} className="spin" /> : <Check size={14} />}{t('aiRoutesApply', { count: selected.size })}</button>
      </div>}
    </div>}
    {result && result.routes.length > visible.length && <p className="route-section-hint"><X size={12} />{t('aiGenerateFiltered', { count: result.routes.length - visible.length })}</p>}
  </Modal>
}
