import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  ArrowUpRight,
  AudioLines,
  Boxes,
  ChevronLeft,
  ChevronRight,
  Film,
  Image as ImageIcon,
  LayoutGrid,
  List,
  MessageSquareText,
  Music2,
  Pencil,
  RefreshCw,
  Search,
  ShieldCheck,
  Sparkles,
  Wand2,
} from 'lucide-react'
import { useWorkspace, useTranslation } from '../workspace'
import { api } from '../api'
import { CopyButton, ProviderMark } from '../components/Provider'
import { DialogActions, Field, Modal, SectionHeading, StatusText, Toggle } from '../components/UI'
import { Dropdown } from '../components/Dropdown'
import {
  familyRoutable,
  filterFamilies,
  formatContext,
  formatPrice,
  formatReferencePrice,
  groupFamilies,
  mapCatalog,
  modelKind,
  paginateFamilies,
  plazaChannelVisible,
  routeForModel,
  type CatalogResponse,
  type PlazaChannel,
  type PlazaFilters,
  type PlazaKind,
  type PlazaModel,
} from '../model-plaza'
import { compileNormalizeRules } from '../model-normalize'
import { strategyLabelKey } from '../route-strategy'

const kindIcons: Record<PlazaKind, typeof MessageSquareText> = {
  chat: MessageSquareText,
  image: ImageIcon,
  audio: AudioLines,
  video: Film,
  embedding: Boxes,
  rerank: List,
  moderation: ShieldCheck,
  music: Music2,
  other: Sparkles,
}

const kindKeys: Record<
  PlazaKind,
  | 'kindChat'
  | 'kindImage'
  | 'kindAudio'
  | 'kindVideo'
  | 'kindEmbedding'
  | 'kindRerank'
  | 'kindModeration'
  | 'kindMusic'
  | 'kindOther'
> = {
  chat: 'kindChat',
  image: 'kindImage',
  audio: 'kindAudio',
  video: 'kindVideo',
  embedding: 'kindEmbedding',
  rerank: 'kindRerank',
  moderation: 'kindModeration',
  music: 'kindMusic',
  other: 'kindOther',
}

function priceRange(
  channels: PlazaChannel[],
  pick: (channel: PlazaChannel) => number | null,
): string {
  const values = channels.map(pick).filter((value): value is number => value !== null)
  if (!values.length) return '—'
  const min = Math.min(...values)
  const max = Math.max(...values)
  return min === max ? formatPrice(min) : `${formatPrice(min)}–${formatPrice(max)}`
}

function PriceEditor({
  channel,
  model,
  onClose,
}: {
  channel: PlazaChannel
  model: string
  onClose: () => void
}) {
  const { notify, run } = useWorkspace()
  const t = useTranslation()
  const [input, setInput] = useState(
    channel.inputPriceMicros === null ? '' : String(channel.inputPriceMicros / 1_000_000),
  )
  const [output, setOutput] = useState(
    channel.outputPriceMicros === null ? '' : String(channel.outputPriceMicros / 1_000_000),
  )
  const [cacheRead, setCacheRead] = useState(
    channel.cacheReadPriceMicros === null ? '' : String(channel.cacheReadPriceMicros / 1_000_000),
  )
  const [saving, setSaving] = useState(false)

  const save = async () => {
    const inputValue = Number(input)
    const outputValue = Number(output)
    const cacheValue = cacheRead.trim() ? Number(cacheRead) : 0
    if (
      !input.trim() ||
      !output.trim() ||
      !Number.isFinite(inputValue) ||
      !Number.isFinite(outputValue) ||
      inputValue <= 0 ||
      outputValue <= 0 ||
      !Number.isFinite(cacheValue) ||
      cacheValue < 0
    ) {
      notify(t('plazaPriceInvalid'))
      return
    }
    setSaving(true)
    try {
      await run(() =>
        api('/api/v1/models/pricing', {
          method: 'POST',
          body: JSON.stringify({
            provider_id: channel.providerId,
            model,
            input_price_micros: Math.round(inputValue * 1_000_000),
            output_price_micros: Math.round(outputValue * 1_000_000),
            cache_read_price_micros: Math.round(cacheValue * 1_000_000),
          }),
        }), ['providers', 'routes'],
      )
      notify(t('plazaPriceSaved'))
      onClose()
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setSaving(false)
    }
  }

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault()
        void save()
      }}
    >
      <Field
        label={`${t('plazaInputPrice')} (USD ${t('plazaPriceUnit')})`}
        value={input}
        onChange={setInput}
        type="number"
        placeholder="1.00"
      />
      <Field
        label={`${t('plazaOutputPrice')} (USD ${t('plazaPriceUnit')})`}
        value={output}
        onChange={setOutput}
        type="number"
        placeholder="2.00"
        hint={t('plazaPriceHint')}
      />
      <Field
        label={`${t('plazaCacheReadPrice')} (USD ${t('plazaPriceUnit')})`}
        value={cacheRead}
        onChange={setCacheRead}
        type="number"
        placeholder="0.10"
        hint={t('plazaCacheReadPriceHint')}
      />
      <DialogActions onClose={onClose} label={t('saveChanges')} disabled={saving} />
    </form>
  )
}

export function PlazaView() {
  const { providers, routes, channelGroups, settings, notify, refresh } = useWorkspace()
  const t = useTranslation()
  const [tab, setTab] = useState<'models' | 'providers'>('models')
  const [catalog, setCatalog] = useState<CatalogResponse | null>(null)
  const [loadError, setLoadError] = useState('')
  const [syncing, setSyncing] = useState(false)
  const [syncWarnings, setSyncWarnings] = useState<string[]>([])
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState<'all' | PlazaKind>('all')
  const [groupId, setGroupId] = useState('all')
  const [providerId, setProviderId] = useState('all')
  const [availableOnly, setAvailableOnly] = useState(true)
  const [routableOnly, setRoutableOnly] = useState(false)
  const [layout, setLayout] = useState<'grid' | 'list'>('grid')
  const [page, setPage] = useState(1)
  const [reloadKey, setReloadKey] = useState(0)
  const [loading, setLoading] = useState(true)
  const [selected, setSelected] = useState<string | null>(null)
  const [priceTarget, setPriceTarget] = useState<{ channel: PlazaChannel; model: string } | null>(
    null,
  )

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      setLoading(true)
      setLoadError('')
      try {
        const response = await api<CatalogResponse>('/api/v1/models/catalog')
        if (!cancelled) {
          setCatalog(response)
          setLoadError('')
        }
      } catch (cause) {
        if (!cancelled) setLoadError(cause instanceof Error ? cause.message : String(cause))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [reloadKey])

  useEffect(() => {
    if (catalog) setReloadKey((current) => current + 1)
  }, [providers, routes])

  const syncMetadata = async () => {
    setSyncing(true)
    setSyncWarnings([])
    try {
      const result = await api<{
        count: number
        prices_filled: number
        warnings?: string[]
      }>('/api/v1/models/metadata/sync', { method: 'POST' })
      const warnings = result.warnings ?? []
      setSyncWarnings(warnings)
      const summary =
        result.prices_filled > 0
          ? `${t('plazaSyncDone', { count: result.count })} ${t('plazaSyncPricesFilled', {
              count: result.prices_filled,
            })}`
          : t('plazaSyncDone', { count: result.count })
      notify(
        warnings.length > 0
          ? `${summary} · ${t('plazaSyncDegraded', { count: warnings.length })}`
          : summary,
      )
      await refresh(undefined, ['providers', 'routes'])
    } catch (cause) {
      notify(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setSyncing(false)
    }
  }

  const groupLabel = (id: string) =>
    id === 'official'
      ? t('officialChannels')
      : id === 'free'
        ? t('freeChannels')
        : id === 'team'
          ? t('teamChannels')
          : channelGroups.find((item) => item.id === id)?.name || t('noGroup')

  const filters: PlazaFilters = { query, kind, groupId, providerId, routableOnly, availableOnly }
  const models = useMemo(() => (catalog ? mapCatalog(catalog.models) : []), [catalog])
  const normalizeRules = useMemo(
    () =>
      compileNormalizeRules({
        routerPrefixes: settings.normalizePrefixes,
        functionalSuffixes: settings.normalizeSuffixes,
        explicitMappings: settings.normalizeMappings,
      }),
    [settings.normalizePrefixes, settings.normalizeSuffixes, settings.normalizeMappings],
  )
  const families = useMemo(() => groupFamilies(models, normalizeRules), [models, normalizeRules])
  const visible = useMemo(
    () => filterFamilies(families, routes, filters),
    [families, routes, query, kind, groupId, providerId, routableOnly, availableOnly],
  )
  const modelPage = paginateFamilies(visible, page, layout === 'grid' ? 48 : 60)
  const selectedFamily = families.find((family) => family.base === selected)
  const resetPage = useCallback(<Value,>(setter: (value: Value) => void) => (value: Value) => {
    setter(value)
    setPage(1)
  }, [])

  const providerOptions = useMemo(() => {
    const seen = new Map<number, string>()
    for (const model of models) {
      for (const channel of model.channels) seen.set(channel.providerId, channel.provider)
    }
    return [...seen.entries()].sort((a, b) => a[1].localeCompare(b[1]))
  }, [models])

  const groupOptions = useMemo(() => {
    const seen = new Set<string>()
    for (const model of models) {
      for (const channel of model.channels) if (channel.groupId) seen.add(channel.groupId)
    }
    return [...seen].sort()
  }, [models])

  const kindLabel = (value: PlazaKind) => t(kindKeys[value])
  const channelStatus = (channel: PlazaChannel) => channel.enabled && channel.keyCount > 0
  const variantChannels = (variant: PlazaModel) =>
    variant.channels.filter((channel) => plazaChannelVisible(channel, filters))

  return (
    <>
      <SectionHeading title={t('plaza')} description={t('plazaSubtitle')} />

      <div className="filter-tabs plaza-tabs">
        {(['models', 'providers'] as const).map((item) => (
          <button
            type="button"
            key={item}
            className={tab === item ? 'active' : ''}
            onClick={() => setTab(item)}
          >
            {item === 'models' ? t('plazaAvailableModels') : t('plazaProvidersTab')}
          </button>
        ))}
      </div>

      {tab === 'models' && (
        <>
          <div className="list-toolbar plaza-toolbar">
            <div className="search-field">
              <Search size={15} />
              <input
                aria-label={t('plazaSearchModels')}
                placeholder={t('plazaSearchModels')}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value)
                  setPage(1)
                }}
              />
            </div>

            <Dropdown
              className="filter-select"
              variant="compact"
              label={t('plazaModelKind')}
              value={kind}
              options={[
                { value: 'all', label: t('plazaAllKinds') },
                ...(Object.keys(kindKeys) as PlazaKind[]).map((value) => ({
                  value,
                  label: kindLabel(value),
                })),
              ]}
              onChange={resetPage((value: string) => setKind(value as 'all' | PlazaKind))}
            />

            <Dropdown
              className="filter-select"
              variant="compact"
              label={t('plazaAllProviders')}
              value={providerId}
              options={[
                { value: 'all', label: t('plazaAllProviders') },
                ...providerOptions.map(([id, name]) => ({ value: String(id), label: name })),
              ]}
              onChange={resetPage(setProviderId)}
            />

            <Dropdown
              className="filter-select"
              variant="compact"
              label={t('allGroups')}
              value={groupId}
              options={[
                { value: 'all', label: t('allGroups') },
                ...groupOptions.map((id) => ({ value: id, label: groupLabel(id) })),
              ]}
              onChange={resetPage(setGroupId)}
            />

            <label className="plaza-switch">
              <Toggle
                checked={availableOnly}
                onChange={() => {
                  setAvailableOnly(!availableOnly)
                  setPage(1)
                }}
                label={t('plazaAvailableOnly')}
              />
              {t('plazaAvailableOnly')}
            </label>

            <label className="plaza-switch">
              <Toggle
                checked={routableOnly}
                onChange={() => {
                  setRoutableOnly(!routableOnly)
                  setPage(1)
                }}
                label={t('plazaRoutableOnly')}
              />
              {t('plazaRoutableOnly')}
            </label>

            <div className="plaza-toolbar-end">
              <button
                type="button"
                className="button secondary plaza-sync"
                onClick={() => void syncMetadata()}
                disabled={syncing}
              >
                <RefreshCw size={14} className={syncing ? 'spin' : ''} />
                {t('plazaSyncMetadata')}
              </button>
              <span className="plaza-count tabular">
                {t('plazaModelCount', { count: visible.length })}
              </span>
              <div className="segmented plaza-layout">
                <button
                  type="button"
                  className={layout === 'grid' ? 'active' : ''}
                  onClick={() => {
                    setLayout('grid')
                    setPage(1)
                  }}
                  aria-label={t('plazaGridView')}
                >
                  <LayoutGrid size={15} />
                </button>
                <button
                  type="button"
                  className={layout === 'list' ? 'active' : ''}
                  onClick={() => {
                    setLayout('list')
                    setPage(1)
                  }}
                  aria-label={t('plazaListView')}
                >
                  <List size={15} />
                </button>
              </div>
            </div>
          </div>

          {catalog?.fetched_at && (
            <div className="plaza-sync-note">
              {t('plazaSyncedAt', { date: catalog.fetched_at })}
              {catalog.source ? ` · ${catalog.source}` : ''}
            </div>
          )}

          {syncWarnings.length > 0 && (
            <div className="plaza-sync-note plaza-sync-warnings" role="status">
              <strong>{t('plazaSyncDegraded', { count: syncWarnings.length })}</strong>
              <ul>
                {syncWarnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            </div>
          )}

          {loadError && (
            <div className="empty-state glass-panel plaza-load-error" role="alert">
              <strong>{t('plazaLoadFailed')}</strong>
              <p>{loadError}</p>
              {loadError.includes('502') && <p>{t('plazaProxyError')}</p>}
              <button
                type="button"
                className="button secondary"
                disabled={loading}
                onClick={() => setReloadKey((current) => current + 1)}
              >
                <RefreshCw size={14} className={loading ? 'spin' : ''} />
                {t('plazaRetry')}
              </button>
            </div>
          )}

          {loading && (
            <div className="empty-state glass-panel">
              <p>{t('plazaLoading')}</p>
            </div>
          )}

          {!loading && !loadError && catalog && !visible.length && (
            <div className="empty-state glass-panel">
              <Wand2 size={22} />
              <strong>{t('plazaEmpty')}</strong>
              <p>{t('plazaEmptyHint')}</p>
            </div>
          )}

          {!loading && !loadError && catalog && modelPage.items.length > 0 && (
            <>
            <div className={layout === 'grid' ? 'plaza-grid' : 'plaza-list'}>
              {modelPage.items.map((family) => {
                const channels = family.channels.filter((channel) =>
                  plazaChannelVisible(channel, filters),
                )
                const familyKind = modelKind(family.base)
                const KindIcon = kindIcons[familyKind]
                const routable = familyRoutable(routes, family)
                const available = channels.some(channelStatus)
                const groups = new Set(channels.map((channel) => channel.groupId))
                const input = priceRange(channels, (channel) => channel.inputPriceMicros)
                const output = priceRange(channels, (channel) => channel.outputPriceMicros)
                const lead = channels[0]
                const leadProvider = lead
                  ? providers.find((item) => item.id === lead.providerId)
                  : undefined
                const refInput = formatReferencePrice(family.metadata?.inputPriceNanos ?? null)
                const refOutput = formatReferencePrice(family.metadata?.outputPriceNanos ?? null)

                return (
                  <button
                    type="button"
                    className={`plaza-card glass-panel ${layout === 'list' ? 'plaza-row' : ''}`}
                    key={family.base}
                    onClick={() => setSelected(family.base)}
                  >
                    <div className="plaza-card-head">
                      <span className={`plaza-kind kind-${familyKind}`}>
                        <KindIcon size={16} />
                      </span>
                      <strong className="plaza-model mono" title={family.displayName}>
                        {family.displayName}
                      </strong>
                      <span onClick={(event) => event.stopPropagation()}>
                        <CopyButton value={family.base} onCopy={() => notify(t('copied'))} />
                      </span>
                    </div>

                    <div className="plaza-card-meta">
                      {lead && (
                        <ProviderMark
                          name={lead.provider}
                          short={leadProvider?.short || lead.provider.slice(0, 2).toUpperCase()}
                          color={leadProvider?.color || '#c4eaaa'}
                          small
                        />
                      )}
                      <span className="meta-token">{kindLabel(familyKind)}</span>
                      {family.variants.length > 1 && (
                        <>
                          <span className="meta-sep" aria-hidden="true">·</span>
                          <span className="meta-token accent-ink">
                            {t('plazaVariantCount', { count: family.variants.length })}
                          </span>
                        </>
                      )}
                      <span className="meta-sep" aria-hidden="true">·</span>
                      <StatusText tone={routable ? 'positive' : 'neutral'}>
                        {routable ? t('plazaRoutable') : t('plazaNotRoutable')}
                      </StatusText>
                      {!available && (
                        <>
                          <span className="meta-sep" aria-hidden="true">·</span>
                          <StatusText tone="danger">{t('plazaUnavailable')}</StatusText>
                        </>
                      )}
                    </div>

                    <div className="plaza-prices tabular">
                      <span>
                        {t('plazaInputPrice')} <strong>{input}</strong>
                        {input !== '—' && <em>{t('plazaPriceUnit')}</em>}
                      </span>
                      <span>
                        {t('plazaOutputPrice')} <strong>{output}</strong>
                        {output !== '—' && <em>{t('plazaPriceUnit')}</em>}
                      </span>
                      {input === '—' && output === '—' && (
                        <span className="meta-token muted">{t('plazaPriceUnset')}</span>
                      )}
                    </div>

                    {family.metadata && (refInput !== '—' || refOutput !== '—') && (
                      <div className="plaza-reference tabular">
                        {t('plazaReferencePrice')} <strong>{refInput}</strong> /{' '}
                        <strong>{refOutput}</strong>
                        {family.metadata.contextLength > 0 && (
                          <em>
                            {t('plazaContext')} {formatContext(family.metadata.contextLength)}
                          </em>
                        )}
                      </div>
                    )}

                    <div className="plaza-card-foot tabular">
                      <span>{t('plazaChannelCount', { count: channels.length })}</span>
                      <span>{t('plazaGroupCount', { count: groups.size })}</span>
                      <ArrowUpRight size={14} />
                    </div>
                  </button>
                )
              })}
            </div>
            <div className="table-pagination plaza-pagination tabular">
              <span>
                {t('plazaPageRange', {
                  start: (modelPage.page - 1) * (layout === 'grid' ? 48 : 60) + 1,
                  end: Math.min(modelPage.page * (layout === 'grid' ? 48 : 60), modelPage.total),
                  total: modelPage.total,
                })}
              </span>
              <div>
                <button
                  type="button"
                  className="icon-button"
                  disabled={modelPage.page === 1}
                  onClick={() => setPage(modelPage.page - 1)}
                  aria-label={t('previousPage')}
                >
                  <ChevronLeft size={15} />
                </button>
                <span>{t('plazaPageNumber', { page: modelPage.page, pages: modelPage.pages })}</span>
                <button
                  type="button"
                  className="icon-button"
                  disabled={modelPage.page === modelPage.pages}
                  onClick={() => setPage(modelPage.page + 1)}
                  aria-label={t('nextPage')}
                >
                  <ChevronRight size={15} />
                </button>
              </div>
            </div>
            </>
          )}
        </>
      )}

      {tab === 'providers' && (
        <div className="plaza-provider-list">
          {providers.length ? (
            providers.map((provider) => {
              const entries = models.filter((model) =>
                model.channels.some((channel) => channel.providerId === provider.id),
              )
              return (
                <div className="plaza-provider-row glass-panel" key={provider.id}>
                  <ProviderMark {...provider} />
                  <div className="plaza-provider-info">
                    <strong>{provider.name}</strong>
                    <span className="tabular">
                      {groupLabel(provider.group)} ·{' '}
                      {t('plazaProviderModels', { count: provider.modelsList.length })}
                    </span>
                  </div>
                  <div className="plaza-provider-models mono">
                    {entries.slice(0, 6).map((entry, idx) => (
                      <span className="meta-token" key={entry.model} title={entry.model}>
                        {idx > 0 && <span className="meta-sep">· </span>}
                        {entry.model}
                      </span>
                    ))}
                    {entries.length > 6 && (
                      <span className="meta-token muted">+{entries.length - 6}</span>
                    )}
                  </div>
                  <StatusText tone={provider.enabled ? 'positive' : 'neutral'}>
                    {provider.enabled ? t('operational') : t('paused')}
                  </StatusText>
                  <button
                    type="button"
                    className="button text-button"
                    onClick={() => {
                      window.location.hash = 'Providers'
                    }}
                  >
                    {t('plazaManageProviders')}
                    <ArrowUpRight size={14} />
                  </button>
                </div>
              )
            })
          ) : (
            <div className="empty-state glass-panel">
              <strong>{t('noProvidersYet')}</strong>
              <p>{t('plazaEmptyHint')}</p>
            </div>
          )}
        </div>
      )}

      <Modal
        open={!!selectedFamily}
        onClose={() => setSelected(null)}
        title={selectedFamily?.displayName || ''}
        description={t('plazaDetailSubtitle')}
        size="route-dialog"
      >
        {selectedFamily && (
          <div className="plaza-detail">
            {selectedFamily.metadata && (
              <div className="plaza-meta glass-panel">
                {selectedFamily.description && (
                  <p className="plaza-meta-desc">{selectedFamily.description}</p>
                )}
                <div className="plaza-meta-facts tabular">
                  {selectedFamily.metadata.contextLength > 0 && (
                    <span>
                      {t('plazaContext')}{' '}
                      <strong>{formatContext(selectedFamily.metadata.contextLength)}</strong>
                    </span>
                  )}
                  {(selectedFamily.metadata.inputPriceNanos !== null ||
                    selectedFamily.metadata.outputPriceNanos !== null) && (
                    <span>
                      {t('plazaReferencePrice')}{' '}
                      <strong>
                        {formatReferencePrice(selectedFamily.metadata.inputPriceNanos)}
                      </strong>{' '}
                      /{' '}
                      <strong>
                        {formatReferencePrice(selectedFamily.metadata.outputPriceNanos)}
                      </strong>
                    </span>
                  )}
                  <span className="plaza-meta-source mono">
                    {selectedFamily.metadata.sourceId}
                  </span>
                </div>
                {selectedFamily.metadata.capabilities.length > 0 && (
                  <div className="plaza-meta-caps">
                    {selectedFamily.metadata.capabilities.join(' · ')}
                  </div>
                )}
              </div>
            )}

            {selectedFamily.variants.map((variant) => {
              const channels = variantChannels(variant)
              const routable = routeForModel(routes, variant.model)
              return (
                <div className="plaza-variant" key={variant.model}>
                  <div className="plaza-variant-head">
                    <strong className="mono" title={variant.model}>
                      {variant.model}
                    </strong>
                    <span className="meta-token">{kindLabel(modelKind(variant.model))}</span>
                    <span className="meta-sep" aria-hidden="true">·</span>
                    <StatusText tone={routable ? 'positive' : 'neutral'}>
                      {routable ? t('plazaRoutable') : t('plazaNotRoutable')}
                    </StatusText>
                    {routable && (
                      <>
                        <span className="meta-sep" aria-hidden="true">·</span>
                        <span className="meta-token">
                          {t('plazaRoutesLabel')}: {routable.name || routable.slug} ·{' '}
                          {t(strategyLabelKey(routable.strategy))}
                        </span>
                      </>
                    )}
                    <span onClick={(event) => event.stopPropagation()}>
                      <CopyButton value={variant.model} onCopy={() => notify(t('copied'))} />
                    </span>
                  </div>

                  {channels.length ? (
                    channels.map((channel) => {
                      const channelProvider = providers.find(
                        (item) => item.id === channel.providerId,
                      )
                      return (
                        <div className="plaza-channel" key={`${variant.model}-${channel.providerId}`}>
                          <div className="plaza-channel-head">
                            <ProviderMark
                              name={channel.provider}
                              short={
                                channelProvider?.short ||
                                channel.provider.slice(0, 2).toUpperCase()
                              }
                              color={channelProvider?.color || '#c4eaaa'}
                              small
                            />
                            <strong>{channel.provider}</strong>
                            <span className="meta-sep" aria-hidden="true">·</span>
                            <span className="meta-token">{groupLabel(channel.groupId)}</span>
                            <span className="plaza-channel-status">
                              <StatusText tone={channelStatus(channel) ? 'positive' : 'neutral'}>
                                {channelStatus(channel)
                                  ? t('plazaAvailable')
                                  : t('plazaUnavailable')}
                              </StatusText>
                            </span>
                          </div>

                          <div className="plaza-channel-meta tabular">
                            <span>{t('plazaKeysServe', { count: channel.keyCount })}</span>
                            <span>
                              {t('plazaInputPrice')}{' '}
                              <strong>{formatPrice(channel.inputPriceMicros)}</strong>
                            </span>
                            <span>
                              {t('plazaOutputPrice')}{' '}
                              <strong>{formatPrice(channel.outputPriceMicros)}</strong>
                            </span>
                            {channel.cacheReadPriceMicros !== null && (
                              <span>
                                {t('plazaCacheReadPrice')}{' '}
                                <strong>{formatPrice(channel.cacheReadPriceMicros)}</strong>
                              </span>
                            )}
                            <button
                              type="button"
                              className="button text-button"
                              onClick={() => setPriceTarget({ channel, model: variant.model })}
                            >
                              <Pencil size={13} />
                              {t('plazaEditPrice')}
                            </button>
                          </div>

                          <div className="plaza-channel-routes">
                            {channel.routes.length ? (
                              channel.routes.map((route, idx) => (
                                <span
                                  className="meta-token accent-ink"
                                  key={route.slug}
                                  title={t(strategyLabelKey(route.strategy))}
                                >
                                  {idx > 0 && <span className="meta-sep">· </span>}
                                  {route.name || route.slug} ({t(strategyLabelKey(route.strategy))})
                                </span>
                              ))
                            ) : (
                              <span className="plaza-route-empty">{t('plazaNoRoutes')}</span>
                            )}
                          </div>
                        </div>
                      )
                    })
                  ) : (
                    <div className="plaza-route-empty">{t('plazaNoVisibleChannels')}</div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </Modal>

      <Modal
        open={!!priceTarget}
        onClose={() => setPriceTarget(null)}
        title={`${t('plazaEditPrice')} · ${priceTarget?.model || ''}`}
        description={priceTarget?.channel.provider}
      >
        {priceTarget && (
          <PriceEditor
            key={`${priceTarget.channel.providerId}-${priceTarget.model}`}
            channel={priceTarget.channel}
            model={priceTarget.model}
            onClose={() => setPriceTarget(null)}
          />
        )}
      </Modal>
    </>
  )
}
