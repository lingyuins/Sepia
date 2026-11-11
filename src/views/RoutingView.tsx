import { useState } from 'react'
import { Activity, ArrowDown, ArrowUp, ArrowUpRight, Copy, Pencil, Play, Plus, ShieldCheck, Sparkles, Trash2, X, Zap } from 'lucide-react'
import { useWorkspace, useTranslation, type Provider, type RouteCandidate, type RouteRule } from '../workspace'
import { api, routeInput } from '../api'
import { DialogActions, Field, Modal, Toggle } from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import { ProviderCell, SectionHeading, StatusDot } from './ProvidersView'

let candidateSequence = 0
const slugify = (value: string, fallback = 'route') => value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || fallback
const modelsForProvider = (provider?: Provider) => provider?.modelsList || []
const makeCandidate = (provider?: Provider): RouteCandidate => ({ id: `candidate-${++candidateSequence}`, provider: provider?.name || '', model: modelsForProvider(provider)[0] || '', enabled: true, weight: 1 })
const routeCandidates = (route: RouteRule): RouteCandidate[] => route.candidates?.length ? route.candidates : [
  { id: `${route.id}-primary`, provider: route.primary, model: route.primaryModel, enabled: true, weight: 1 },
  ...(route.fallback ? [{ id: `${route.id}-fallback`, provider: route.fallback, model: route.fallbackModel, enabled: true, weight: 1 }] : []),
]

function useRouteMeta() {
  const t = useTranslation()
  const strategyName = (_value: string) => t('failover')
  const strategyDescription = (value: string) => value === 'Sequential' ? t('strategySequentialDescription') : value === 'Round robin' ? t('strategyRoundRobinDescription') : value === 'Least used' ? t('strategyLeastUsedDescription') : value === 'Lowest latency' ? t('strategyLatencyDescription') : value === 'Cost optimized' ? t('strategyCostDescription') : value === 'Weighted' ? t('strategyWeightedDescription') : t('strategySmartDescription')
  return { t, strategyName, strategyDescription }
}

type ChoiceOption = { value: string; label: string }
function ChoiceRow({ label, value, options, onChange, hint, children }: { label: string; value: string; options: ChoiceOption[]; onChange: (value: string) => void; hint?: string; children?: React.ReactNode }) {
  return <div className="route-choice-row"><div className="route-choice-label">{label}</div><div className="route-choice-content"><div className="route-choice-options">{options.map((option) => <button type="button" className={value === option.value ? 'active' : ''} key={option.value} onClick={() => onChange(option.value)}>{option.label}</button>)}</div>{children}{hint && <p>{hint}</p>}</div></div>
}

function RouteForm({ route, onClose }: { route: RouteRule | null; onClose: () => void }) {
  const { providers, run, notify } = useWorkspace()
  const { t, strategyDescription } = useRouteMeta()
  const modelsFor = (name: string) => { const provider = providers.find((item) => item.name === name); return provider ? modelsForProvider(provider) : [] }
  const [name, setName] = useState(route?.name || '')
  const [slug, setSlug] = useState(route?.slug || (route ? slugify(route.name) : 'new-route'))
  const [description, setDescription] = useState(route?.description || '')
  const [candidates, setCandidates] = useState<RouteCandidate[]>(() => route ? routeCandidates(route).map((candidate) => ({ ...candidate })) : providers.length ? [makeCandidate(providers[0]), ...(providers[1] ? [makeCandidate(providers[1])] : [])] : [])
  const [patterns, setPatterns] = useState<string[]>(route?.patterns || (route ? [route.match] : []))
  const [patternDraft, setPatternDraft] = useState('')
  const [modelListing, setModelListing] = useState(route?.listMembers === false ? 'group' : 'members')
  const [strategy, setStrategy] = useState(route?.strategy === 'Smart routing' ? 'Failover' : route?.strategy || 'Failover')
  const [active, setActive] = useState(route?.active ?? true)
  const names = providers.map((provider) => provider.name)
  const updateCandidate = (id: string, patch: Partial<RouteCandidate>) => setCandidates((current) => current.map((candidate) => candidate.id === id ? { ...candidate, ...patch } : candidate))
  const changeCandidateProvider = (id: string, provider: string) => updateCandidate(id, { provider, model: modelsFor(provider)[0] || '' })
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
      strategy, active, requests: route?.requests || '0', candidates: candidates.map((candidate) => ({ ...candidate, weight: Math.max(1, candidate.weight || 1) })), patterns: finalPatterns,
      listMembers: modelListing === 'members', isDefault: route?.isDefault || false,
    }
    try { await run(() => api(route ? `/api/v1/routes/${route.id}` : '/api/v1/routes', { method: route ? 'PUT' : 'POST', body: JSON.stringify(routeInput(saved, providers)) })); notify(route ? t('routeUpdated') : t('routeCreated')); onClose() } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) }
  }
  const strategyOptions = [
    { value: 'Failover', label: t('failover') },
  ]
  return <form className="route-editor" onSubmit={save}>
    <section className="route-editor-section">
      <div className="route-editor-section-title"><h3>{t('routeGroup')}</h3></div>
      <div className="form-grid"><Field label={t('routeName')} value={name} onChange={(value) => { setName(value); if (!route) setSlug(slugify(value, 'new-route')) }} required placeholder={t('routeNamePlaceholder')} /><Field label={t('routeId')} value={slug} onChange={setSlug} required placeholder="auto-route" /></div>
      <Field label={t('description')} value={description} onChange={setDescription} placeholder={t('routeDescriptionPlaceholder')} />
      <div className="route-id-preview"><span>{t('agentModelId')}</span><code>{slug || 'route'}</code></div>
    </section>
    <section className="route-editor-section">
      <div className="route-editor-section-title"><h3>{t('candidateModels')}</h3><span>{candidates.filter((candidate) => candidate.enabled).length} / {candidates.length}</span></div>
      <div className="route-candidate-editor">{candidates.map((candidate, index) => {
        const provider = providers.find((item) => item.name === candidate.provider)
        return <div className={`route-candidate-row ${candidate.enabled ? '' : 'disabled'}`} key={candidate.id}>
          <Toggle checked={candidate.enabled} onChange={() => updateCandidate(candidate.id, { enabled: !candidate.enabled })} label={t('toggleNamed', { name: candidate.model || candidate.provider })} />
          <span className="route-candidate-index">{index + 1}</span>
          <div className="candidate-selects">
            <Dropdown className="candidate-dropdown" variant="compact" label={t('candidateProvider', { number: index + 1 })} value={candidate.provider} options={names.map((providerName) => ({ value: providerName, label: providerName }))} onChange={(value) => changeCandidateProvider(candidate.id, value)} />
            <Dropdown className="candidate-dropdown" variant="compact" label={t('candidateModel', { number: index + 1 })} value={candidate.model} options={modelsFor(candidate.provider).map((model) => ({ value: model, label: model }))} onChange={(value) => updateCandidate(candidate.id, { model: value })} />
          </div>
          {!!provider?.latency && <span className="candidate-latency">{provider.latency} ms</span>}
          {strategy === 'Weighted' && <label className="candidate-weight"><span>{t('weight')}</span><input className="field" type="number" min="1" max="100" value={candidate.weight} onChange={(event) => updateCandidate(candidate.id, { weight: Number(event.target.value) })} /></label>}
          <div className="candidate-actions"><button type="button" className="icon-button" disabled={index === 0} onClick={() => moveCandidate(index, -1)} aria-label={t('moveCandidateUp', { name: candidate.model })}><ArrowUp size={14} /></button><button type="button" className="icon-button" disabled={index === candidates.length - 1} onClick={() => moveCandidate(index, 1)} aria-label={t('moveCandidateDown', { name: candidate.model })}><ArrowDown size={14} /></button><button type="button" className="icon-button danger-icon" onClick={() => setCandidates((current) => current.filter((item) => item.id !== candidate.id))} aria-label={t('removeCandidate', { name: candidate.model })}><X size={14} /></button></div>
        </div>
      })}</div>
      <button type="button" className="add-candidate-button" disabled={!providers.length} onClick={() => setCandidates((current) => [...current, makeCandidate(providers[0])])}><Plus size={14} />{t('addAnotherModel')}</button>
      <p className="route-section-hint">{t('candidateOrderHint')}</p>
    </section>
    <section className="route-editor-section">
      <div className="route-editor-section-title"><h3>{t('matchPatterns')}</h3></div>
      <div className="pattern-input-row"><input className="field" value={patternDraft} onChange={(event) => setPatternDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addPattern() } }} placeholder={t('patternPlaceholder')} aria-label={t('patternPlaceholder')} /><button type="button" className="button secondary" onClick={addPattern}><Plus size={13} />{t('addPattern')}</button></div>
      {!!patterns.length && <div className="model-chip-list">{patterns.map((pattern) => <span className="model-chip" key={pattern}><code>{pattern}</code><button type="button" onClick={() => setPatterns((current) => current.filter((item) => item !== pattern))} aria-label={t('removePattern', { name: pattern })}><X size={12} /></button></span>)}</div>}
      <p className="route-section-hint">{t('patternHint')}</p>
    </section>
    <section className="route-editor-section">
      <div className="route-editor-section-title"><h3>{t('routingStrategy')}</h3></div>
      <ChoiceRow label={t('modelList')} value={modelListing} options={[{ value: 'members', label: t('listMemberModels') }, { value: 'group', label: t('listGroupOnly') }]} onChange={setModelListing} hint={t('modelListHint')} />
      <ChoiceRow label={t('routingStrategy')} value={strategy} options={strategyOptions} onChange={setStrategy} hint={strategyDescription(strategy)} />
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
  const [testing, setTesting] = useState<RouteRule | null>(null)
  const [query, setQuery] = useState('')
  const filtered = routes.filter((route) => `${route.name} ${route.primary} ${route.match} ${routeCandidates(route).map((candidate) => `${candidate.provider} ${candidate.model}`).join(' ')}`.toLowerCase().includes(query.toLowerCase()))
  const updateRoute = (id: number, enabled: boolean) => { run(() => api(`/api/v1/routes/${id}/enabled`, { method: 'POST', body: JSON.stringify({ enabled }) })).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))) }
  const close = () => { setOpen(false); setEditing(null) }
  const testingCandidates = testing ? routeCandidates(testing).filter((candidate) => candidate.enabled) : []
  return <>
    <SectionHeading eyebrow={t('routingEyebrow')} title={t('routingTitle')} description={t('routingSubtitle')} action={<button className="button primary" onClick={() => setOpen(true)}><Plus size={16} />{t('createRoute')}</button>} />
    <div className="routing-principles"><div><span><Zap size={19} /></span><h3>{t('fasterDefault')}</h3><p>{t('fasterDescription')}</p></div><div><span><ShieldCheck size={19} /></span><h3>{t('reliableNature')}</h3><p>{t('reliableDescription')}</p></div><div><span><Sparkles size={19} /></span><h3>{t('smarterPlugins')}</h3><p>{t('smarterDescription')}</p></div></div>
    <div className="list-toolbar"><div className="search-field"><input aria-label={t('findRoute')} placeholder={t('findRoute')} value={query} onChange={(event) => setQuery(event.target.value)} /></div><span>{routes.filter((route) => route.active).length} {t('activeShort')} · {t('default')}: {strategyName('Failover')}</span></div>
    {filtered.length ? <div className="routes-list">{filtered.map((route) => {
      const candidates = routeCandidates(route).filter((candidate) => candidate.enabled)
      return <div className={`route-card glass-panel ${!route.active ? 'is-paused' : ''}`} key={route.id}>
        <div className="route-card-head"><div><span className="route-strategy">{strategyName(route.strategy)}</span><h3>{route.name || route.slug || `Route ${route.id}`}</h3><p>{route.description || t('defaultRouteDescription')}</p></div><Toggle checked={route.active} onChange={() => updateRoute(route.id, !route.active)} label={t('toggleNamed', { name: route.name })} /></div>
        <div className="route-match"><span>{t('whenModelMatches')}</span><code>{route.patterns?.join(' · ') || route.match}</code></div>
        <div className="route-candidates-preview">{candidates.map((candidate, index) => <div className="route-candidate-preview" key={candidate.id}><span className="route-candidate-index">{index + 1}</span><ProviderCell name={candidate.provider} /><code>{candidate.model}</code>{route.strategy === 'Weighted' && <span className="candidate-latency">{t('weight')} {candidate.weight}</span>}{index === 0 && <small>{t('primary')}</small>}</div>)}</div>
        <div className="route-card-foot"><span><Activity size={13} />{route.requests} {t('requestsRouted')}</span><span className={route.active ? 'positive' : 'muted'}><StatusDot />{route.active ? t('routeActive') : t('routePaused')}</span><div className="card-actions"><button className="icon-button" onClick={() => setTesting(route)} aria-label={t('testNamed', { name: route.name })}><Play size={14} /></button><button className="icon-button" onClick={() => { const copy = { ...route, id: Date.now(), name: `${route.name} ${t('copySuffix')}`, slug: `${route.slug || slugify(route.name)}-copy`, requests: '0', isDefault: false }; run(() => api('/api/v1/routes', { method: 'POST', body: JSON.stringify(routeInput(copy, providers)) })).then(() => notify(t('routeDuplicated'))).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))) }} aria-label={t('duplicateNamed', { name: route.name })}><Copy size={14} /></button><button className="icon-button" onClick={() => { setEditing(route); setOpen(true) }} aria-label={t('editNamed', { name: route.name })}><Pencil size={14} /></button><button className="icon-button danger-icon" onClick={() => setDeleting(route)} aria-label={t('deleteNamed', { name: route.name })}><Trash2 size={14} /></button></div></div>
      </div>
    })}</div> : <div className="empty-state glass-panel">{t('noRouteMatch')}<button className="button text-button" onClick={() => setQuery('')}>{t('clearSearch')}</button></div>}
    <Modal open={open} onClose={close} title={editing ? t('refineRoute') : t('createSmarterRoute')} description={t('routeDialogSubtitle')} size="route-dialog"><RouteForm key={editing?.id || 'new'} route={editing} onClose={close} /></Modal>
    <Modal open={!!deleting} onClose={() => setDeleting(null)} title={t('removeRouteTitle')} description={t('removeRouteDescription')}><form onSubmit={(event) => { event.preventDefault(); if (deleting) run(() => api(`/api/v1/routes/${deleting.id}`, { method: 'DELETE' })).then(() => notify(t('routeRemoved'))).catch((cause) => notify(cause instanceof Error ? cause.message : String(cause))); setDeleting(null) }}><DialogActions onClose={() => setDeleting(null)} label={t('removeRoute')} /></form></Modal>
    <Modal open={!!testing} onClose={() => setTesting(null)} title={t('routeTestTitle')} description={t('routeTestSubtitle')}><div className="route-test-result"><div><span className="test-success"><CheckIcon />{t('requestMatched')}</span><code>{testing?.patterns?.join(' · ') || testing?.match}</code></div>{testingCandidates.map((candidate, index) => <div className="test-step" key={candidate.id}><span>{index + 1}</span><div><strong>{index === 0 ? t('primaryCandidate') : t('candidateNumber', { number: index + 1 })}</strong><p>{candidate.provider} · {candidate.model}</p></div><small>{providers.find((provider) => provider.name === candidate.provider)?.latency ? `${providers.find((provider) => provider.name === candidate.provider)?.latency} ms` : t('healthy')}</small></div>)}<div className="secure-note"><ShieldCheck size={15} />{t('noUpstream')}</div></div><div className="dialog-actions"><button className="button primary" onClick={() => setTesting(null)}>{t('looksGood')}<ArrowUpRight size={14} /></button></div></Modal>
  </>
}
function CheckIcon() { return <span className="check-icon">✓</span> }