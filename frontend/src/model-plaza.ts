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
export type PlazaModel = { model: string; channels: PlazaChannel[] }
export type ApiCatalogModel = {
  model: string
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

export function mapCatalog(entries: ApiCatalogModel[]): PlazaModel[] {
  return entries.map((entry) => ({
    model: entry.model,
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

// Mirrors backend/src/server.rs `glob`: '*' is the only wildcard.
export function globMatch(pattern: string, value: string): boolean {
  let p = 0
  let v = 0
  let star = -1
  let retry = 0
  while (v < value.length) {
    if (p < pattern.length && pattern[p] === value[v]) { p += 1; v += 1 }
    else if (p < pattern.length && pattern[p] === '*') { star = p; p += 1; retry = v }
    else if (star >= 0) { retry += 1; v = retry; p = star + 1 }
    else return false
  }
  while (p < pattern.length && pattern[p] === '*') p += 1
  return p === pattern.length
}

// Mirrors backend/src/server.rs `route_for_model`: slug, exact candidate,
// pattern, then default route. Only enabled routes participate.
export function routeForModel(routes: RouteRule[], model: string): RouteRule | undefined {
  const active = routes.filter((route) => route.active)
  const candidatesOf = (route: RouteRule) => route.candidates || []
  return active.find((route) => route.slug === model)
    || active.find((route) => candidatesOf(route).some((candidate) => candidate.enabled && candidate.model === model))
    || active.find((route) => (route.patterns || [route.match]).some((pattern) => globMatch(pattern, model)))
    || active.find((route) => route.isDefault)
}

export function modelRoutable(routes: RouteRule[], model: string): boolean {
  return routeForModel(routes, model) !== undefined
}

export type PlazaKind = 'chat' | 'image' | 'audio' | 'video' | 'embedding' | 'rerank' | 'moderation' | 'music' | 'other'

// Heuristic naming conventions only; Sepia stores no capability metadata.
export function modelKind(model: string): PlazaKind {
  const id = model.toLowerCase()
  if (/(rerank)/.test(id)) return 'rerank'
  if (/(embedding|embed-|bge-|text-embedding)/.test(id)) return 'embedding'
  if (/(moderation|omni-moderation)/.test(id)) return 'moderation'
  if (/(suno|music|lyrics)/.test(id)) return 'music'
  if (/(sora|video|kling|veo|cogvideo|wan-|hunyuan-video)/.test(id)) return 'video'
  if (/(whisper|tts|audio|speech|transcrib|realtime)/.test(id)) return 'audio'
  if (/(image|dall-e|dalle|flux|midjourney|stable-diffusion|sd-|imagen|gpt-image|seedream|cogview)/.test(id)) return 'image'
  if (/(gpt|claude|gemini|grok|deepseek|qwen|glm|llama|mistral|o[134](-|$)|chat|command|kimi|moonshot|doubao|ernie|hunyuan|minimax|yi-|phi-|gemma)/.test(id)) return 'chat'
  return 'other'
}

export type PlazaFilters = {
  query: string
  kind: 'all' | PlazaKind
  groupId: string
  providerId: number | 'all'
  routableOnly: boolean
  availableOnly: boolean
}

export function filterPlaza(models: PlazaModel[], routes: RouteRule[], filters: PlazaFilters): PlazaModel[] {
  const query = filters.query.trim().toLowerCase()
  return models.filter((entry) => {
    if (query && !entry.model.toLowerCase().includes(query) && !entry.channels.some((channel) => channel.provider.toLowerCase().includes(query))) return false
    if (filters.kind !== 'all' && modelKind(entry.model) !== filters.kind) return false
    if (filters.routableOnly && !modelRoutable(routes, entry.model)) return false
    const channels = entry.channels.filter((channel) =>
      (filters.groupId === 'all' || channel.groupId === filters.groupId)
      && (filters.providerId === 'all' || channel.providerId === filters.providerId))
    if (!channels.length) return false
    if (filters.availableOnly && !channels.some((channel) => channel.enabled && channel.keyCount > 0)) return false
    return true
  })
}

export function plazaChannelVisible(channel: PlazaChannel, filters: PlazaFilters): boolean {
  return (filters.groupId === 'all' || channel.groupId === filters.groupId)
    && (filters.providerId === 'all' || channel.providerId === filters.providerId)
}

export function formatPrice(micros: number | null): string {
  if (micros === null) return '—'
  const value = micros / 1_000_000
  if (value >= 100) return `$${value.toFixed(0)}`
  if (value >= 1) return `$${value.toFixed(2)}`
  return `$${value.toFixed(4).replace(/0+$/, '').replace(/\.$/, '.0')}`
}