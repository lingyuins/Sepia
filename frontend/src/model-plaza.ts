import type { RouteRule } from './workspace'

export type PlazaChannel = {
  providerId: number
  provider: string
  groupId: string
  protocol: string
  enabled: boolean
  keyCount: number
  inputPriceMicros: number | null
  outputPriceMicros: number | null
  routes: { slug: string; name: string }[]
}

export type ModelMeta = {
  sourceId: string
  baseModel: string
  displayName: string
  description: string
  contextLength: number
  inputModalities: string[]
  outputModalities: string[]
  capabilities: string[]
  inputPriceNanos: number | null
  outputPriceNanos: number | null
}

export type PlazaModel = {
  model: string
  baseModel: string
  metadata: ModelMeta | null
  channels: PlazaChannel[]
}

export type PlazaFamily = {
  base: string
  displayName: string
  description: string
  metadata: ModelMeta | null
  variants: PlazaModel[]
  channels: PlazaChannel[]
}

export type ApiCatalogModel = {
  model: string
  base_model: string
  metadata: {
    source_id: string
    base_model: string
    display_name: string
    description: string
    context_length: number
    input_modalities: string[]
    output_modalities: string[]
    capabilities: string[]
    input_price_nanos: number | null
    output_price_nanos: number | null
  } | null
  channels: {
    provider_id: number
    provider: string
    group_id: string
    protocol: string
    enabled: boolean
    key_count: number
    input_price_micros: number | null
    output_price_micros: number | null
    routes: { slug: string; name: string }[]
  }[]
}

export type CatalogResponse = {
  source: string | null
  fetched_at: string | null
  models: ApiCatalogModel[]
}

export function mapCatalog(entries: ApiCatalogModel[]): PlazaModel[] {
  return entries.map((entry) => ({
    model: entry.model,
    baseModel: entry.base_model,
    metadata: entry.metadata
      ? {
          sourceId: entry.metadata.source_id,
          baseModel: entry.metadata.base_model,
          displayName: entry.metadata.display_name,
          description: entry.metadata.description,
          contextLength: entry.metadata.context_length,
          inputModalities: entry.metadata.input_modalities,
          outputModalities: entry.metadata.output_modalities,
          capabilities: entry.metadata.capabilities,
          inputPriceNanos: entry.metadata.input_price_nanos,
          outputPriceNanos: entry.metadata.output_price_nanos,
        }
      : null,
    channels: entry.channels.map((channel) => ({
      providerId: channel.provider_id,
      provider: channel.provider,
      groupId: channel.group_id,
      protocol: channel.protocol,
      enabled: channel.enabled,
      keyCount: channel.key_count,
      inputPriceMicros: channel.input_price_micros,
      outputPriceMicros: channel.output_price_micros,
      routes: channel.routes,
    })),
  }))
}

// Group local models into families by the backend-normalized base id so dated /
// free / preview variants of one model collapse into a single plaza card.
export function groupFamilies(models: PlazaModel[]): PlazaFamily[] {
  const byBase = new Map<string, PlazaModel[]>()
  for (const model of models) {
    const list = byBase.get(model.baseModel) ?? []
    list.push(model)
    byBase.set(model.baseModel, list)
  }
  const families: PlazaFamily[] = []
  for (const [base, variants] of byBase) {
    variants.sort((a, b) => a.model.localeCompare(b.model))
    const withMeta = variants.find((v) => v.metadata) ?? null
    const metadata = withMeta?.metadata ?? null
    const channels = variants.flatMap((v) => v.channels)
    families.push({
      base,
      displayName: metadata?.displayName || base,
      description: metadata?.description ?? '',
      metadata,
      variants,
      channels,
    })
  }
  families.sort((a, b) => a.base.localeCompare(b.base))
  return families
}

export function globMatch(pattern: string, value: string): boolean {
  if (pattern === '*') return true
  let p = 0
  let v = 0
  let star = -1
  let mark = 0
  while (v < value.length) {
    if (p < pattern.length && pattern[p] === '*') {
      star = p
      mark = v
      p += 1
    } else if (p < pattern.length && pattern[p] === value[v]) {
      p += 1
      v += 1
    } else if (star >= 0) {
      p = star + 1
      v = mark + 1
      mark = v
    } else return false
  }
  while (p < pattern.length && pattern[p] === '*') p += 1
  return p === pattern.length
}

export function routeForModel(routes: RouteRule[], model: string): RouteRule | null {
  const enabled = routes.filter((route) => route.active)
  const exact = enabled.find((route) => route.slug === model)
  if (exact) return exact
  const candidate = enabled.find((route) =>
    route.candidates?.some((c) => c.enabled && c.model === model),
  )
  if (candidate) return candidate
  for (const route of enabled) {
    if (route.patterns?.some((pattern) => globMatch(pattern, model))) return route
  }
  return enabled.find((route) => route.isDefault) ?? null
}

export function modelRoutable(routes: RouteRule[], model: PlazaModel): boolean {
  return routeForModel(routes, model.model) !== null
}
export function familyRoutable(routes: RouteRule[], family: PlazaFamily): boolean {
  return family.variants.some((v) => modelRoutable(routes, v))
}

export function modelKind(model: string): PlazaKind {
  const id = model.toLowerCase()
  if (/rerank|reranker/.test(id)) return 'rerank'
  if (/embed|embedding|text-embedding/.test(id)) return 'embedding'
  if (/moderation|omni-moderation/.test(id)) return 'moderation'
  if (/suno|music|lyrics/.test(id)) return 'music'
  if (/^dall-e|image|flux|stable-diffusion|midjourney|sd-|sdxl|gpt-image/.test(id)) return 'image'
  if (/^whisper|tts|speech|audio|transcrib/.test(id)) return 'audio'
  if (/^sora|video|kling|runway|gen-|luma|veo/.test(id)) return 'video'
  return 'chat'
}

export type PlazaKind = 'chat' | 'image' | 'audio' | 'video' | 'embedding' | 'rerank' | 'moderation' | 'music' | 'other'

export type PlazaFilters = {
  query: string
  kind: string
  groupId: string
  providerId: string
  routableOnly: boolean
  availableOnly: boolean
}

export function plazaChannelVisible(channel: PlazaChannel, filters: PlazaFilters): boolean {
  if (filters.groupId && filters.groupId !== 'all' && String(channel.groupId ?? '') !== filters.groupId) return false
  if (filters.providerId && filters.providerId !== 'all' && String(channel.providerId) !== filters.providerId) return false
  if (filters.availableOnly && !(channel.enabled && channel.keyCount > 0)) return false
  return true
}

export function filterFamilies(
  families: PlazaFamily[],
  routes: RouteRule[],
  filters: PlazaFilters,
): PlazaFamily[] {
  const query = filters.query.trim().toLowerCase()
  return families
    .map((family) => {
      const variants = family.variants.filter((variant) => {
        if (filters.kind && filters.kind !== 'all' && modelKind(variant.model) !== filters.kind) return false
        if (filters.routableOnly && !modelRoutable(routes, variant)) return false
        const channels = variant.channels.filter((c) => plazaChannelVisible(c, filters))
        if (channels.length === 0) return false
        if (query) {
          const names = [variant.model, variant.baseModel, family.displayName, ...channels.map((c) => c.provider)]
          if (!names.some((n) => n.toLowerCase().includes(query))) return false
        }
        return true
      })
      if (variants.length === 0) return null
      return { ...family, variants, channels: variants.flatMap((v) => v.channels) }
    })
    .filter((f): f is PlazaFamily => f !== null)
}

export function formatPrice(micros: number | null): string {
  if (micros === null) return '—'
  const perMillion = micros / 1_000_000
  if (perMillion >= 100) return `$${perMillion.toFixed(0)}`
  if (perMillion >= 1) return `$${perMillion.toFixed(2)}`
  return `$${perMillion.toFixed(4).replace(/0+$/, '').replace(/\.$/, '')}`
}

// Reference prices from the metadata source are nanos per token (1e9 nanos per
// USD), so per-million USD = nanos / 1000, with up to 3 decimals.
export function formatReferencePrice(nanos: number | null): string {
  if (nanos === null) return '—'
  const perMillion = nanos / 1_000
  if (perMillion >= 100) return `$${perMillion.toFixed(0)}`
  if (perMillion >= 1) return `$${perMillion.toFixed(2)}`
  return `$${perMillion.toFixed(3).replace(/0+$/, '').replace(/\.$/, '')}`
}

export function formatContext(length: number): string {
  if (length <= 0) return '—'
  if (length >= 1_000_000) return `${(length / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`
  if (length >= 1_000) return `${Math.round(length / 1_000)}K`
  return String(length)
}