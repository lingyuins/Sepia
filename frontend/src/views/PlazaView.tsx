import { useEffect, useMemo, useState } from 'react'
import { ArrowUpRight, AudioLines, Boxes, Film, Image as ImageIcon, LayoutGrid, List, MessageSquareText, Music2, Pencil, Search, ShieldCheck, Sparkles, Wand2 } from 'lucide-react'
import { useWorkspace, useTranslation } from '../workspace'
import { api } from '../api'
import { CopyButton, ProviderMark } from '../components/Provider'
import { DialogActions, Field, Modal, SectionHeading, StatusDot, Toggle } from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import { filterPlaza, formatPrice, mapCatalog, modelKind, modelRoutable, plazaChannelVisible, routeForModel, type PlazaChannel, type PlazaFilters, type PlazaKind, type PlazaModel } from '../model-plaza'

const kindIcons: Record<PlazaKind, typeof MessageSquareText> = { chat: MessageSquareText, image: ImageIcon, audio: AudioLines, video: Film, embedding: Boxes, rerank: List, moderation: ShieldCheck, music: Music2, other: Sparkles }
const kindKeys: Record<PlazaKind, 'kindChat' | 'kindImage' | 'kindAudio' | 'kindVideo' | 'kindEmbedding' | 'kindRerank' | 'kindModeration' | 'kindMusic' | 'kindOther'> = { chat: 'kindChat', image: 'kindImage', audio: 'kindAudio', video: 'kindVideo', embedding: 'kindEmbedding', rerank: 'kindRerank', moderation: 'kindModeration', music: 'kindMusic', other: 'kindOther' }

function priceRange(channels: PlazaChannel[], pick: (channel: PlazaChannel) => number | null): string {
  const values = channels.map(pick).filter((value): value is number => value !== null)
  if (!values.length) return '—'
  const min = Math.min(...values)
  const max = Math.max(...values)
  return min === max ? formatPrice(min) : `${formatPrice(min)}–${formatPrice(max)}`
}

function PriceEditor({ channel, model, onClose }: { channel: PlazaChannel; model: string; onClose: () => void }) {
  const { notify, run } = useWorkspace()
  const t = useTranslation()
  const [input, setInput] = useState(channel.inputPriceMicros === null ? '' : String(channel.inputPriceMicros / 1_000_000))
  const [output, setOutput] = useState(channel.outputPriceMicros === null ? '' : String(channel.outputPriceMicros / 1_000_000))
  const [saving, setSaving] = useState(false)
  const save = async () => {
    const inputValue = Number(input)
    const outputValue = Number(output)
    if (!input.trim() || !output.trim() || !Number.isFinite(inputValue) || !Number.isFinite(outputValue) || inputValue <= 0 || outputValue <= 0) { notify(t('plazaPriceInvalid')); return }
    setSaving(true)
    try {
      await run(() => api('/api/v1/models/pricing', { method: 'POST', body: JSON.stringify({ provider_id: channel.providerId, model, input_price_micros: Math.round(inputValue * 1_000_000), output_price_micros: Math.round(outputValue * 1_000_000) }) }))
      notify(t('plazaPriceSaved'))
      onClose()
    } catch (cause) { notify(cause instanceof Error ? cause.message : String(cause)) } finally { setSaving(false) }
  }
  return <form onSubmit={(event) => { event.preventDefault(); void save() }}>
    <Field label={`${t('plazaInputPrice')} (USD ${t('plazaPriceUnit')})`} value={input} onChange={setInput} type="number" placeholder="1.00" />
    <Field label={`${t('plazaOutputPrice')} (USD ${t('plazaPriceUnit')})`} value={output} onChange={setOutput} type="number" placeholder="2.00" hint={t('plazaPriceHint')} />
    <DialogActions onClose={onClose} label={t('saveChanges')} disabled={saving} />
  </form>
}

export function PlazaView() {
  const { providers, routes, channelGroups, notify } = useWorkspace()
  const t = useTranslation()
  const [tab, setTab] = useState<'models' | 'providers'>('models')
  const [catalog, setCatalog] = useState<PlazaModel[] | null>(null)
  const [loadError, setLoadError] = useState('')
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState<'all' | PlazaKind>('all')
  const [groupId, setGroupId] = useState('all')
  const [providerId, setProviderId] = useState<number | 'all'>('all')
  const [availableOnly, setAvailableOnly] = useState(true)
  const [routableOnly, setRoutableOnly] = useState(false)
  const [layout, setLayout] = useState<'grid' | 'list'>('grid')
  const [selected, setSelected] = useState<string | null>(null)
  const [priceTarget, setPriceTarget] = useState<{ channel: PlazaChannel; model: string } | null>(null)
  useEffect(() => {
    let cancelled = false
    api<Parameters<typeof mapCatalog>[0]>('/api/v1/models/catalog')
      .then((entries) => { if (!cancelled) { setCatalog(mapCatalog(entries)); setLoadError('') } })
      .catch((cause) => { if (!cancelled) setLoadError(cause instanceof Error ? cause.message : String(cause)) })
    return () => { cancelled = true }
  }, [providers, routes])
  const groupLabel = (id: string) => id === 'official' ? t('officialChannels') : id === 'free' ? t('freeChannels') : id === 'team' ? t('teamChannels') : channelGroups.find((item) => item.id === id)?.name || t('noGroup')
  const filters: PlazaFilters = { query, kind, groupId, providerId, routableOnly, availableOnly }
  const visible = useMemo(() => catalog ? filterPlaza(catalog, routes, filters) : [], [catalog, routes, query, kind, groupId, providerId, routableOnly, availableOnly])
  const selectedModel = catalog?.find((entry) => entry.model === selected)
  const selectedChannels = selectedModel ? selectedModel.channels.filter((channel) => plazaChannelVisible(channel, filters)) : []
  const providerOptions = useMemo(() => {
    const seen = new Map<number, string>()
    for (const entry of catalog || []) for (const channel of entry.channels) seen.set(channel.providerId, channel.provider)
    return [...seen.entries()].sort((a, b) => a[1].localeCompare(b[1]))
  }, [catalog])
  const groupOptions = useMemo(() => {
    const seen = new Set<string>()
    for (const entry of catalog || []) for (const channel of entry.channels) seen.add(channel.groupId)
    return [...seen].sort()
  }, [catalog])
  const kindLabel = (value: PlazaKind) => t(kindKeys[value])
  const channelStatus = (channel: PlazaChannel) => channel.enabled && channel.keyCount > 0
  return <>
    <SectionHeading title={t('plaza')} description={t('plazaSubtitle')} />
    <div className="filter-tabs plaza-tabs">{(['models', 'providers'] as const).map((item) => <button key={item} className={tab === item ? 'active' : ''} onClick={() => setTab(item)}>{item === 'models' ? t('plazaAvailableModels') : t('plazaProvidersTab')}</button>)}</div>
    {tab === 'models' && <>
      <div className="list-toolbar plaza-toolbar">
        <div className="search-field"><Search size={15} /><input aria-label={t('plazaSearchModels')} placeholder={t('plazaSearchModels')} value={query} onChange={(event) => setQuery(event.target.value)} /></div>
        <Dropdown className="filter-select" variant="compact" label={t('plazaModelKind')} value={kind} options={[{ value: 'all', label: t('plazaAllKinds') }, ...(Object.keys(kindKeys) as PlazaKind[]).map((value) => ({ value, label: kindLabel(value) }))]} onChange={(value) => setKind(value as 'all' | PlazaKind)} />
        <Dropdown className="filter-select" variant="compact" label={t('plazaAllProviders')} value={String(providerId)} options={[{ value: 'all', label: t('plazaAllProviders') }, ...providerOptions.map(([id, name]) => ({ value: String(id), label: name }))]} onChange={(value) => setProviderId(value === 'all' ? 'all' : Number(value))} />
        <Dropdown className="filter-select" variant="compact" label={t('allGroups')} value={groupId} options={[{ value: 'all', label: t('allGroups') }, ...groupOptions.map((id) => ({ value: id, label: groupLabel(id) }))]} onChange={setGroupId} />
        <label className="plaza-switch"><Toggle checked={availableOnly} onChange={() => setAvailableOnly(!availableOnly)} label={t('plazaAvailableOnly')} />{t('plazaAvailableOnly')}</label>
        <label className="plaza-switch"><Toggle checked={routableOnly} onChange={() => setRoutableOnly(!routableOnly)} label={t('plazaRoutableOnly')} />{t('plazaRoutableOnly')}</label>
        <div className="plaza-toolbar-end"><span className="plaza-count">{t('plazaModelCount', { count: visible.length })}</span><div className="segmented plaza-layout"><button className={layout === 'grid' ? 'active' : ''} onClick={() => setLayout('grid')} aria-label={t('plazaGridView')}><LayoutGrid size={15} /></button><button className={layout === 'list' ? 'active' : ''} onClick={() => setLayout('list')} aria-label={t('plazaListView')}><List size={15} /></button></div></div>
      </div>
      {loadError && <div className="empty-state glass-panel"><strong>{t('plazaLoadFailed')}</strong><p>{loadError}</p></div>}
      {!loadError && !catalog && <div className="empty-state glass-panel"><p>{t('plazaLoading')}</p></div>}
      {catalog && !visible.length && <div className="empty-state glass-panel"><Wand2 size={22} /><strong>{t('plazaEmpty')}</strong><p>{t('plazaEmptyHint')}</p></div>}
      {catalog && visible.length > 0 && <div className={layout === 'grid' ? 'plaza-grid' : 'plaza-list'}>{visible.map((entry) => {
        const channels = entry.channels.filter((channel) => plazaChannelVisible(channel, filters))
        const entryKind = modelKind(entry.model)
        const KindIcon = kindIcons[entryKind]
        const routable = modelRoutable(routes, entry.model)
        const available = channels.some(channelStatus)
        const groups = new Set(channels.map((channel) => channel.groupId))
        const input = priceRange(channels, (channel) => channel.inputPriceMicros)
        const output = priceRange(channels, (channel) => channel.outputPriceMicros)
        const lead = channels[0]
        return <button type="button" className={`plaza-card glass-panel ${layout === 'list' ? 'plaza-row' : ''}`} key={entry.model} onClick={() => setSelected(entry.model)}>
          <div className="plaza-card-head"><span className={`plaza-kind kind-${entryKind}`}><KindIcon size={16} /></span><strong className="plaza-model" title={entry.model}>{entry.model}</strong><span onClick={(event) => event.stopPropagation()}><CopyButton value={entry.model} onCopy={() => notify(t('copied'))} /></span></div>
          <div className="plaza-card-meta">{lead && <ProviderMark name={lead.provider} short={providers.find((item) => item.id === lead.providerId)?.short || lead.provider.slice(0, 2).toUpperCase()} color={providers.find((item) => item.id === lead.providerId)?.color || '#c4f4aa'} small />}<span className="plaza-chip">{kindLabel(entryKind)}</span><span className={`plaza-chip ${routable ? 'chip-ok' : 'chip-muted'}`}>{routable ? t('plazaRoutable') : t('plazaNotRoutable')}</span>{!available && <span className="plaza-chip chip-warn">{t('plazaUnavailable')}</span>}</div>
          <div className="plaza-prices"><span>{t('plazaInputPrice')} <strong>{input}</strong>{input !== '—' && <em>{t('plazaPriceUnit')}</em>}</span><span>{t('plazaOutputPrice')} <strong>{output}</strong>{output !== '—' && <em>{t('plazaPriceUnit')}</em>}</span>{input === '—' && output === '—' && <span className="plaza-chip chip-muted">{t('plazaPriceUnset')}</span>}</div>
          <div className="plaza-card-foot"><span>{t('plazaChannelCount', { count: channels.length })}</span><span>{t('plazaGroupCount', { count: groups.size })}</span><ArrowUpRight size={14} /></div>
        </button>
      })}</div>}
    </>}
    {tab === 'providers' && <div className="plaza-provider-list">{providers.length ? providers.map((provider) => {
      const entries = (catalog || []).filter((entry) => entry.channels.some((channel) => channel.providerId === provider.id))
      return <div className="plaza-provider-row glass-panel" key={provider.id}>
        <ProviderMark {...provider} />
        <div className="plaza-provider-info"><strong>{provider.name}</strong><span>{groupLabel(provider.group)} · {t('plazaProviderModels', { count: provider.modelsList.length })}</span></div>
        <div className="plaza-provider-models">{entries.slice(0, 6).map((entry) => <span className="plaza-chip" key={entry.model} title={entry.model}>{entry.model}</span>)}{entries.length > 6 && <span className="plaza-chip chip-muted">+{entries.length - 6}</span>}</div>
        <span className={provider.enabled ? 'positive' : 'muted'}><StatusDot />{provider.enabled ? t('operational') : t('paused')}</span>
        <button className="button text-button" onClick={() => { window.location.hash = 'Providers' }}>{t('plazaManageProviders')}<ArrowUpRight size={14} /></button>
      </div>
    }) : <div className="empty-state glass-panel"><strong>{t('noProvidersYet')}</strong><p>{t('plazaEmptyHint')}</p></div>}</div>}
    <Modal open={!!selectedModel} onClose={() => setSelected(null)} title={selectedModel?.model || ''} description={t('plazaDetailSubtitle')}>
      {selectedModel && <div className="plaza-detail">
        <div className="plaza-detail-tags"><span className="plaza-chip">{kindLabel(modelKind(selectedModel.model))}</span><span className={`plaza-chip ${modelRoutable(routes, selectedModel.model) ? 'chip-ok' : 'chip-muted'}`}>{modelRoutable(routes, selectedModel.model) ? t('plazaRoutable') : t('plazaNotRoutable')}</span>{routeForModel(routes, selectedModel.model) && <span className="plaza-chip">{t('plazaRoutesLabel')}: {routeForModel(routes, selectedModel.model)?.name || routeForModel(routes, selectedModel.model)?.slug}</span>}</div>
        {selectedChannels.map((channel) => <div className="plaza-channel" key={channel.providerId}>
          <div className="plaza-channel-head"><ProviderMark name={channel.provider} short={providers.find((item) => item.id === channel.providerId)?.short || channel.provider.slice(0, 2).toUpperCase()} color={providers.find((item) => item.id === channel.providerId)?.color || '#c4f4aa'} small /><strong>{channel.provider}</strong><span className="plaza-chip">{groupLabel(channel.groupId)}</span><span className={channelStatus(channel) ? 'positive' : 'muted'}><StatusDot />{channelStatus(channel) ? t('plazaAvailable') : t('plazaUnavailable')}</span></div>
          <div className="plaza-channel-meta"><span>{t('plazaKeysServe', { count: channel.keyCount })}</span><span>{t('plazaInputPrice')} <strong>{formatPrice(channel.inputPriceMicros)}</strong></span><span>{t('plazaOutputPrice')} <strong>{formatPrice(channel.outputPriceMicros)}</strong></span><button className="button text-button" onClick={() => setPriceTarget({ channel, model: selectedModel.model })}><Pencil size={13} />{t('plazaEditPrice')}</button></div>
          <div className="plaza-channel-routes">{channel.routes.length ? channel.routes.map((route) => <span className="plaza-chip chip-ok" key={route.slug}>{route.name || route.slug}</span>) : <span className="plaza-route-empty">{t('plazaNoRoutes')}</span>}</div>
        </div>)}
      </div>}
    </Modal>
    <Modal open={!!priceTarget} onClose={() => setPriceTarget(null)} title={`${t('plazaEditPrice')} · ${priceTarget?.model || ''}`} description={priceTarget?.channel.provider}>
      {priceTarget && <PriceEditor key={`${priceTarget.channel.providerId}-${priceTarget.model}`} channel={priceTarget.channel} model={priceTarget.model} onClose={() => setPriceTarget(null)} />}
    </Modal>
  </>
}