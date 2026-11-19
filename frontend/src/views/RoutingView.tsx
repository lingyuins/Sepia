import { useEffect, useMemo, useState } from 'react'
import { Activity, ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, Copy, GripVertical, Pencil, Play, Plus, RefreshCw, Search, Sparkles, Trash2, X } from 'lucide-react'
import { useWorkspace, useTranslation, type Provider, type RouteCandidate, type RouteRule } from '../workspace'
import { api, routeInput } from '../api'
import { DialogActions, Field, Modal, Toggle, SectionHeading, StatusDot } from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import { RouteChain } from '../components/RouteChain'
import { routeDisplayCandidates } from '../route-display'
import { AiRouteDialog } from '../components/AiRouteDialog'
import { ROUTE_STRATEGY_IDS, normalizeRouteStrategy, strategyDescriptionKey, strategyLabelKey, strategyUsesWeights, clampWeight, type RouteStrategyId } from '../route-strategy'

type ProbeResult = { ok: boolean; latency_ms: number; endpoint: string | null; error: string | null; reply: string | null }
type RouteTestResponse = { route: string; candidates: { provider_id: number; provider: string | null; model: string; enabled: boolean; result: ProbeResult }[] }

let candidateSequence = 0
const slugify = (value: string, fallback = 'route') => value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || fallback
const modelsForProvider = (provider?: Provider) => provider?.modelsList || []
const makeCandidate = (provider?: Provider): RouteCandidate => ({ id: `candidate-${++candidateSequence}`, provider: provider?.name || '', model: modelsForProvider(provider)[0] || '', enabled: true, weight: 1 })
const routeCandidates = routeDisplayCandidates

function useRouteMeta() {
  const t = useTranslation()
  const strategyName = (value: string) => t(strategyLabelKey(value))
  return { t, strategyName }
}

function RouteForm({ route, onClose }: { route: RouteRule | null; onClose: () => void }) {
  const { providers, run, notify } = useWorkspace()
  const { t } = useRouteMeta()
  const modelsFor = (name: string) => { const provider = providers.find((item) => item.name === name); return provider ? modelsForProvider(provider) : [] }
  const [name, setName] = useState(route?.name || '')
  const [slug, setSlug] = useState(route?.slug || (route ? slugify(route.name) : 'new-route'))
  const [description, setDescription] = useState(route?.description || '')
  const [candidates, setCandidates] = useState<RouteCandidate[]>(() => route ? routeCandidates(route).filter((candidate) => candidate.enabled).map((candidate) => ({ ...candidate })) : [])
  const [expandedProvider, setExpandedProvider] = useState(providers[0]?.name || '')
  const [modelQuery, setModelQuery] = useState('')
  const [patterns, setPatterns] = useState<string[]>(route?.patterns || (route ? [route.match] : []))
  const [patternDraft, setPatternDraft] = useState('')
  const [modelListing, setModelListing] = useState(route?.listMembers === false ? 'group' : 'members')
  const [strategy, setStrategy] = useState<RouteStrategyId>(normalizeRouteStrategy(route?.strategy))
  const [active, setActive] = useState(route?.active ?? true)
  const updateCandidate = (id: string, patch: Partial<RouteCandidate>) => setCandidates((current) => current.map((candidate) => candidate.id === id ? { ...candidate, ...patch } : candidate))
  const changeCandidateProvider = (id: string, provider: string) => updateCandidate(id, { provider, model: modelsFor(provider)[0] || '' })
  const toggleCandidate = (provider: Provider, model: string) => setCandidates((current) => {
    const existing = current.find((candidate) => candidate.provider === provider.name && candidate.model === model)
    if (existing) return current.filter((candidate) => candidate.id !== existing.id)
    return [...current, { ...makeCandidate(provider), model }]
  })
  const dragCandidate = (event: React.DragEvent, index: number) => { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', String(index)) }
  const dropCandidate = (event: React.DragEvent, index: number) => {
    event.preventDefault()
    const source = Number(event.dataTransfer.getData('text/plain'))
    if (!Number.isInteger(source) || source === index) return
    setCandidates((current) => { const next = [...current]; const [candidate] = next.splice(source, 1); next.splice(index, 0, candidate); return next })
  }
  const moveCandidate = (index: number, direction: -1 | 1) => setCandidates((current) => {
    const target = index + direction
    if (target < 0 || target >= current.length) return current
    const next = [...current]
    const [candidate] = next.splice(index, 1)
    next.splice(target, 0, candidate)
    return next
  })
  const addPattern = () => {
    const pattern = patternDraft.trim()
    if (!pattern) { notify(t('patternRequired')); return }
    if (patterns.includes(pattern)) { notify(t('patternExists')); return }
    setPatterns((current) => [...current, pattern])
    setPatternDraft('')
  }
  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    const enabled = candidates.filter((candidate) => candidate.enabled && candidate.model)
    if (!enabled.length) { notify(t('routeNeedsModel')); return }
    const finalPatterns = patterns.length ? patterns : ['*']
    const saved: RouteRule = {
      id: route?.id || Date.now(), name: name.trim(), slug: slug.trim() || slugify(name), description: description.trim() || t('defaultRouteDescription'),
      match: finalPatterns[0], primary: enabled[0].provider, primaryModel: enabled[0].model, fallback: enabled[1]?.provider || '', fallbackModel: enabled[1]?.model || '',
      strategy, active, requests: route?.requests || '0', candidates: enabled.map((candidate) => ({ ...candidate, weight: clampWeight(candidate.weight || 1) })), patterns: finalPatterns,
      listMembers: modelListing === 'members', isDefault: route?.isDefault || false,
    }
    try { await run(() => api(route ? `/api/v1/routes/${route.id}` : '/api/v1/routes', { method: route ? 'PUT' : 'POST', body: JSON.stringify(routeInput(saved, providers)) })); notify(route ? t('routeUpdated') : t('routeCreated')); onClose() } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  return <form className="route-editor" onSubmit={save}>
    <section className="route-editor-section">
      <div className="route-editor-section-title"><h3>{t('routeGroup')}</h3></div>
      <div className="form-grid"><Field label={t('routeName')} value={name} onChange={(value) => { setName(value); if (!route) setSlug(slugify(value, 'new-route')) }} required placeholder={t('routeNamePlaceholder')} /><Field label={t('routeId')} value={slug} onChange={setSlug} required placeholder="auto-route" hint={`${t('agentModelId')}: ${slug || 'route'}`} /></div>
      <Field label={t('description')} value={description} onChange={setDescription} placeholder={t('routeDescriptionPlaceholder')} />
      <div className="route-group-options">
        <div className="route-group-field"><span>{t('routingStrategy')}</span><Dropdown variant="compact" label={t('routingStrategy')} value={strategy} options={ROUTE_STRATEGY_IDS.map((id) => ({ value: id, label: t(strategyLabelKey(id)) }))} onChange={(value) => setStrategy(normalizeRouteStrategy(value))} /></div>
        <div className="route-group-field"><span>{t('modelList')}</span><Dropdown variant="compact" label={t('modelList')} value={modelListing} options={[{ value: 'members', label: t('listMemberModels') }, { value: 'group', label: t('listGroupOnly') }]} onChange={setModelListing} /></div>
      </div>
      <p className="route-section-hint">{t(strategyDescriptionKey(strategy))} {t('strategyFailoverNote')} {t('modelListHint')}</p>
      <div className="route-group-patterns">
        <span className="route-group-label">{t('matchPatterns')}</span>
        <div className="pattern-input-row"><input className="field" value={patternDraft} onChange={(event) => setPatternDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addPattern() } }} placeholder={t('patternPlaceholder')} aria-label={t('patternPlaceholder')} /><button type="button" className="button secondary" onClick={addPattern}><Plus size={13} />{t('addPattern')}</button></div>
        {!!patterns.length && <div className="model-chip-list">{patterns.map((pattern) => <span className="model-chip" key={pattern}><code>{pattern}</code><button type="button" onClick={() => setPatterns((current) => current.filter((item) => item !== pattern))} aria-label={t('removePattern', { name: pattern })}><X size={12} /></button></span>)}</div>}
        <p className="route-section-hint">{t('patternHint')}</p>
      </div>
    </section>
    <section className="route-editor-section route-candidate-section">
      <div className="route-editor-section-title"><h3>{t('candidateModels')}</h3><span>{candidates.length}</span></div>
      <div className="route-candidate-workspace">
        <div className="candidate-catalog">
          <label className="candidate-search"><Search size={14} /><input value={modelQuery} onChange={(event) => setModelQuery(event.target.value)} placeholder={t('searchModels')} aria-label={t('searchModels')} /></label>
          <div className="candidate-provider-list">{providers.map((provider) => {
            const models = modelsForProvider(provider).filter((model) => model.toLowerCase().includes(modelQuery.trim().toLowerCase()))
            const expanded = expandedProvider === provider.name
            return <div className={`candidate-provider ${expanded ? 'expanded' : ''}`} key={provider.id}>
              <button type="button" className="candidate-provider-toggle" aria-expanded={expanded} onClick={() => setExpandedProvider(expanded ? '' : provider.name)}><span>{provider.name}</span><small>{models.length}/{modelsForProvider(provider).length}</small><ChevronDown size={13} /></button>
              {expanded && <div className="candidate-provider-models">{models.length ? models.map((model) => {
                const selected = candidates.some((candidate) => candidate.provider === provider.name && candidate.model === model)
                return <button type="button" className={selected ? 'selected' : ''} aria-pressed={selected} key={model} onClick={() => toggleCandidate(provider, model)}><span title={model}>{model}</span>{selected && <Check size={13} />}</button>
              }) : <span className="candidate-no-models">{t('noMatchingModels')}</span>}</div>}
            </div>
          })}</div>
          <button type="button" className="add-candidate-button" disabled={!providers.some((provider) => modelsForProvider(provider).length)} onClick={() => { const next = providers.flatMap((provider) => modelsForProvider(provider).map((model) => ({ provider, model }))).find(({ provider, model }) => !candidates.some((candidate) => candidate.provider === provider.name && candidate.model === model)); if (next) toggleCandidate(next.provider, next.model) }}><Plus size={13} />{t('addAnotherModel')}</button>
        </div>
        <div className="candidate-selection">
          <div className="candidate-selection-heading"><strong>{t('selectedModels')}</strong><span>{candidates.length}</span><button type="button" disabled={!candidates.length} onClick={() => setCandidates([])}><Trash2 size={13} />{t('clear')}</button></div>
          <div className="route-candidate-editor">{candidates.map((candidate, index) => <div className="route-candidate-row" key={candidate.id} onDragOver={(event) => event.preventDefault()} onDrop={(event) => dropCandidate(event, index)}>
            <span className="route-candidate-index">{index + 1}</span>
            <span className="candidate-drag-handle" draggable onDragStart={(event) => dragCandidate(event, index)} aria-label={t('moveCandidateUp', { name: candidate.model })}><GripVertical size={15} /></span>
            <div className="candidate-card-copy">
              <Dropdown className="candidate-dropdown" variant="compact" label={t('candidateModel', { number: index + 1 })} value={candidate.model} options={modelsFor(candidate.provider).map((model) => ({ value: model, label: model }))} onChange={(value) => updateCandidate(candidate.id, { model: value })} />
              <Dropdown className="candidate-provider-dropdown" variant="compact" label={t('candidateProvider', { number: index + 1 })} value={candidate.provider} options={providers.map((provider) => ({ value: provider.name, label: provider.name }))} onChange={(value) => changeCandidateProvider(candidate.id, value)} />
            </div>
            {strategyUsesWeights(strategy) && <label className="candidate-weight"><span>{t('candidateWeight')}</span><input type="number" min={1} max={100} value={candidate.weight} aria-label={t('candidateWeightNamed', { name: candidate.model })} onChange={(event) => { const raw = event.target.value; if (raw === '') return; const parsed = Number(raw); if (Number.isFinite(parsed)) updateCandidate(candidate.id, { weight: Math.round(parsed) }) }} onBlur={() => updateCandidate(candidate.id, { weight: clampWeight(candidate.weight) })} /></label>}
            <div className="candidate-actions"><button type="button" className="icon-button" disabled={index === 0} onClick={() => moveCandidate(index, -1)} aria-label={t('moveCandidateUp', { name: candidate.model })}><ArrowUp size={13} /></button><button type="button" className="icon-button" disabled={index === candidates.length - 1} onClick={() => moveCandidate(index, 1)} aria-label={t('moveCandidateDown', { name: candidate.model })}><ArrowDown size={13} /></button><button type="button" className="icon-button danger-icon" onClick={() => setCandidates((current) => current.filter((item) => item.id !== candidate.id))} aria-label={t('removeCandidate', { name: candidate.model })}><X size={14} /></button></div>
          </div>)}</div>
          {!candidates.length && <div className="candidate-selection-empty">{t('routeNeedsModel')}</div>}
          <p className="route-section-hint">{t('candidateOrderHint')}</p>
          {strategyUsesWeights(strategy) && !!candidates.length && <p className="route-section-hint">{t('strategyWeightHint')}</p>}
        </div>
      </div>
    </section>
    <div className="switch-row"><span>{t('enableRoute')}</span><Toggle checked={active} onChange={() => setActive(!active)} label={t('enableRoute')} /></div>
    <DialogActions onClose={onClose} label={route ? t('saveRoute') : t('createRoute')} />
  </form>
}

export function RoutingView() {
  const { routes, run, notify, providers } = useWorkspace()
  const { t, strategyName } = useRouteMeta()
  const [editing, setEditing] = useState<RouteRule | null>(null)
  const [open, setOpen] = useState(false)
  const [deleting, setDeleting] = useState<RouteRule | null>(null)
  const [aiOpen, setAiOpen] = useState(false)
  const [testing, setTesting] = useState<RouteRule | null>(null)
  const [testResults, setTestResults] = useState<RouteTestResponse | null>(null)
  const [testError, setTestError] = useState('')
  const [testRunning, setTestRunning] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedId, setSelectedId] = useState<number>()
  const filtered = useMemo(() => routes.filter((route) => `${route.name} ${route.slug || ''} ${route.primary} ${route.match} ${routeDisplayCandidates(route).map((candidate) => `${candidate.provider} ${candidate.model}`).join(' ')}`.toLowerCase().includes(query.toLowerCase())), [routes, query])
  const selectedRoute = filtered.find((route) => route.id === selectedId) || filtered[0]
  useEffect(() => { if (selectedId && !filtered.some((route) => route.id === selectedId)) setSelectedId(undefined) }, [filtered, selectedId])
  const updateRoute = (id: number, enabled: boolean) => { run(() => api(`/api/v1/routes/${id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled }) })).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))) }
  const close = () => { setOpen(false); setEditing(null) }
  const closeTest = () => { setTesting(null); setTestResults(null); setTestError('') }
  const runRouteTest = async (route: RouteRule) => {
    setTesting(route); setTestResults(null); setTestError(''); setTestRunning(true)
    try { setTestResults(await api<RouteTestResponse>(`/api/v1/routes/${route.id}/test`, { method: 'POST' })) }
    catch (cause) { setTestError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setTestRunning(false) }
  }
  return <>
    <SectionHeading title={t('routingTitle')} description={t('routingSubtitle')} action={<><button className="button secondary" onClick={() => setAiOpen(true)}><Sparkles size={16} />{t('aiGenerateTitle')}</button><button className="button primary" onClick={() => setOpen(true)}><Plus size={16} />{t('createRoute')}</button></>} />
    {filtered.length && selectedRoute ? <div className="route-workspace"><section className="route-list glass-panel" aria-label={t('routeList')}><div className="route-list-heading"><h2>{t('routeList')}</h2><span>{filtered.length}</span></div>{filtered.map((route) => <button className={`route-list-item ${selectedRoute.id === route.id ? 'selected' : ''}`} key={route.id} onClick={() => setSelectedId(route.id)}><span className="route-list-name">{route.name || route.slug}<ChevronRight size={15} /></span><code>{route.slug}</code><span className={route.active ? 'positive' : 'muted'}><StatusDot />{t('candidatesCount', { count: routeDisplayCandidates(route).length })} · {route.active ? t('routeActive') : t('routePaused')}</span></button>)}</section>
      <section className={`route-detail glass-panel ${!selectedRoute.active ? 'is-paused' : ''}`}><div className="route-detail-head"><div><div className="route-detail-title"><h2>{selectedRoute.name || selectedRoute.slug}</h2><span className={selectedRoute.active ? 'positive' : 'muted'}><StatusDot />{selectedRoute.active ? t('routeActive') : t('routePaused')}</span></div><p>{selectedRoute.description || t('defaultRouteDescription')}</p><code>{selectedRoute.slug} · {strategyName(selectedRoute.strategy)}</code></div><Toggle checked={selectedRoute.active} onChange={() => updateRoute(selectedRoute.id, !selectedRoute.active)} label={t('toggleNamed', { name: selectedRoute.name })} /></div><div className="route-match"><span>{t('whenModelMatches')}</span><code>{selectedRoute.patterns?.join(' · ') || selectedRoute.match}</code></div><RouteChain route={selectedRoute} /><p className="route-preview-hint">{t('routePreviewHint')}</p><div className="route-detail-foot"><span><Activity size={14} />{selectedRoute.requests} {t('requestsRouted')}</span><div className="card-actions"><button className="button secondary" onClick={() => runRouteTest(selectedRoute)}><Play size={14} />{t('testRoute')}</button><button className="icon-button" onClick={() => { const copy = { ...selectedRoute, id: Date.now(), name: `${selectedRoute.name} ${t('copySuffix')}`, slug: `${selectedRoute.slug || slugify(selectedRoute.name)}-copy`, requests: '0', isDefault: false }; run(() => api('/api/v1/routes', { method: 'POST', body: JSON.stringify(routeInput(copy, providers)) })).then(() => notify(t('routeDuplicated'))).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))) }} aria-label={t('duplicateNamed', { name: selectedRoute.name })}><Copy size={15} /></button><button className="icon-button" onClick={() => { setEditing(selectedRoute); setOpen(true) }} aria-label={t('editNamed', { name: selectedRoute.name })}><Pencil size={15} /></button><button className="icon-button danger-icon" onClick={() => setDeleting(selectedRoute)} aria-label={t('deleteNamed', { name: selectedRoute.name })}><Trash2 size={15} /></button></div></div></section></div> : <div className="empty-state glass-panel">{t('noRouteMatch')}<button className="button text-button" onClick={() => setQuery('')}>{t('clearSearch')}</button></div>}
    <Modal open={open} onClose={close} title={editing ? t('refineRoute') : t('createSmarterRoute')} description={t('routeDialogSubtitle')} size="route-dialog"><RouteForm key={editing?.id || 'new'} route={editing} onClose={close} /></Modal>
    <Modal open={!!deleting} onClose={() => setDeleting(null)} title={t('removeRouteTitle')} description={t('removeRouteDescription')}><form onSubmit={(event) => { event.preventDefault(); if (deleting) run(() => api(`/api/v1/routes/${deleting.id}`, { method: 'DELETE' })).then(() => notify(t('routeRemoved'))).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))); setDeleting(null) }}><DialogActions onClose={() => setDeleting(null)} label={t('removeRoute')} /></form></Modal>
    <Modal open={!!testing} onClose={closeTest} title={t('routeTestTitle')} description={t('routeTestSubtitle')}>
      <div className="route-test-result">
        <div><span className="test-match-label">{t('whenModelMatches')}</span><code>{testing?.patterns?.join(' · ') || testing?.match}</code></div>
        {testRunning && <p className="route-test-running"><RefreshCw size={14} className="spin" />{t('routeTestRunning')}</p>}
        {testError && <p className="route-test-error">{testError}</p>}
        {testResults?.candidates.map((candidate, index) => <div className="test-step" key={`${candidate.provider_id}-${candidate.model}-${index}`}>
          <span>{index + 1}</span>
          <div><strong>{candidate.provider || `#${candidate.provider_id}`} · {candidate.model}</strong>
            {candidate.result.ok
              ? <p className="test-ok"><Check size={13} />{t('routeTestAvailable')}{candidate.result.reply ? ` · ${candidate.result.reply}` : ''}</p>
              : <p className="test-fail"><X size={13} />{candidate.result.error || t('routeTestUnavailable')}</p>}
          </div>
          <small>{candidate.result.endpoint ? `${candidate.result.endpoint} · ${candidate.result.latency_ms} ms` : `${candidate.result.latency_ms} ms`}</small>
        </div>)}
        <p className="route-section-hint">{t('routeTestNote')}</p>
        {testing && <div className="route-test-actions"><button type="button" className="button secondary" disabled={testRunning} onClick={() => runRouteTest(testing)}><RefreshCw size={13} className={testRunning ? 'spin' : ''} />{t('routeTestRerun')}</button></div>}
      </div>
    </Modal>
    <AiRouteDialog open={aiOpen} onClose={() => setAiOpen(false)} />
  </>
}
