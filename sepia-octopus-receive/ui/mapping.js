// Octopus backup → Sepia management-API request mapping (pure, testable).
// All functions run in the sandboxed plugin UI; the guest only forwards the
// resulting {path, method, body} requests through the admin.request capability.
// Provider/route references use names in the plan and are resolved to ids at
// execution time (resolveCandidates), because ids only exist after creation.

var OctopusMapping = (function () {
function slugify(name) {
  const slug = String(name || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 64)
  return slug || 'imported'
}

function uniqueSlug(name, used) {
  const base = slugify(name)
  let slug = base
  let n = 2
  while (used.has(slug)) slug = `${base}-${n++}`
  used.add(slug)
  return slug
}

// Octopus outbound type → Sepia provider protocol. 1 = OpenAI Responses;
// everything else in the backup (0/5/6) is OpenAI-compatible chat.
function protocolFor(type) {
  return type === 1 ? 'openai-responses' : 'openai'
}

function splitModels(value) {
  return String(value || '')
    .split(',')
    .map((model) => model.trim())
    .filter(Boolean)
}

// $/M tokens → integer micros/token (round to nearest, minimum 1 when priced).
function priceMicros(value) {
  const amount = Number(value)
  if (!Number.isFinite(amount) || amount <= 0) return 0
  return Math.max(1, Math.round(amount * 1_000_000))
}

// Octopus group mode → Sepia routing strategy id.
function strategyFor(mode) {
  switch (mode) {
    case 1: return 'round-robin'
    case 2: return 'random'
    case 4: return 'weighted'
    default: return 'priority' // 3 failover, 5 auto
  }
}

function baseUrls(channel) {
  const urls = Array.isArray(channel.base_urls) ? channel.base_urls : []
  return urls.map((entry) => (entry && entry.url ? String(entry.url) : '')).filter(Boolean)
}

function channelKeys(backup, channelId) {
  return (backup.channel_keys || [])
    .filter((key) => key.channel_id === channelId && key.enabled !== false && key.channel_key)
    .sort((a, b) => (a.priority || 0) - (b.priority || 0))
}

// channels + channel_keys + channel_groups → group/provider create requests.
// providerMap maps Octopus channel id → the Sepia provider name used later.
function mapProviders(backup, existingProviders, existingGroups) {
  const requests = []
  const providerMap = new Map()
  const usedNames = new Set((existingProviders || []).map((provider) => provider.name))
  const knownGroups = new Set((existingGroups || []).map((group) => group.id))

  for (const group of backup.channel_groups || []) {
    const id = `octopus-${group.id}`
    if (!knownGroups.has(id)) {
      requests.push({ path: '/api/v1/groups', method: 'POST', body: { id, name: group.name, description: '' }, kind: 'group' })
      knownGroups.add(id)
    }
  }

  for (const channel of backup.channels || []) {
    const urls = baseUrls(channel)
    const keys = channelKeys(backup, channel.id)
    const models = splitModels(channel.model)
    if (urls.length === 0 || keys.length === 0 || models.length === 0) continue
    let name = String(channel.name || `channel-${channel.id}`).trim() || `channel-${channel.id}`
    if (usedNames.has(name)) name = `${name} (${channel.id})`
    usedNames.add(name)
    providerMap.set(channel.id, name)
    const groupId = channel.group_id != null ? `octopus-${channel.group_id}` : 'official'
    const protocol = protocolFor(channel.type)
    const enabled = channel.enabled !== false
    if (urls.length === 1) {
      requests.push({
        path: '/api/v1/providers', method: 'POST', kind: 'provider', name,
        body: { name, protocol, endpoint: urls[0], models, secret: keys[0].channel_key, secrets: keys.map((key) => key.channel_key), group_id: groupId, enabled, mode: 'single' },
      })
    } else {
      requests.push({
        path: '/api/v1/providers', method: 'POST', kind: 'provider', name,
        body: {
          name, protocol, endpoint: urls[0], models, group_id: groupId, enabled, mode: 'unified',
          endpoints: urls.map((url, index) => ({
            endpoint: url,
            secrets: index === 0 ? keys.map((key) => key.channel_key) : [keys[0].channel_key],
          })),
        },
      })
    }
  }
  return { requests, providerMap }
}

// groups + group_items → route create requests. Candidates carry provider
// names; resolveCandidates turns them into [provider_id, model] tuples.
function mapRoutes(backup, providerMap, existingRoutes) {
  const requests = []
  const usedSlugs = new Set((existingRoutes || []).map((route) => route.slug))
  const itemsByGroup = new Map()
  for (const item of backup.group_items || []) {
    const list = itemsByGroup.get(item.group_id) || []
    list.push(item)
    itemsByGroup.set(item.group_id, list)
  }
  for (const group of backup.groups || []) {
    const items = (itemsByGroup.get(group.id) || []).slice().sort((a, b) => (a.priority || 0) - (b.priority || 0))
    const candidates = []
    const weights = []
    for (const item of items) {
      const providerName = providerMap.get(item.channel_id)
      if (!providerName || !item.model_name) continue
      candidates.push({ provider: providerName, model: String(item.model_name) })
      weights.push(Math.min(100, Math.max(1, Number(item.weight) || 1)))
    }
    if (candidates.length === 0) continue
    const slug = uniqueSlug(group.name, usedSlugs)
    const strategy = strategyFor(group.mode)
    requests.push({
      path: '/api/v1/routes', method: 'POST', kind: 'route', slug,
      body: {
        slug, name: String(group.name || slug), description: '',
        patterns: [slug], candidates, strategy,
        weights: strategy === 'weighted' ? weights : undefined,
        list_members: true, enabled: true,
      },
    })
  }
  return requests
}

// Resolve a route request's name-based candidates into API tuples using the
// live provider list. Returns null when any candidate provider is missing.
function resolveCandidates(request, providers) {
  const byName = new Map(providers.map((provider) => [provider.name, provider.id]))
  const tuples = []
  for (const candidate of request.body.candidates) {
    const id = byName.get(candidate.provider)
    if (id == null) return null
    tuples.push([id, candidate.model])
  }
  return { ...request, body: { ...request.body, candidates: tuples } }
}

// llm_infos → pricing requests keyed by provider name + model.
function mapPrices(backup, providerMap) {
  const prices = new Map()
  for (const info of backup.llm_infos || []) {
    const input = priceMicros(info.input)
    const output = priceMicros(info.output)
    if (input > 0 && output > 0) prices.set(String(info.name), { input, output })
  }
  const requests = []
  for (const channel of backup.channels || []) {
    const providerName = providerMap.get(channel.id)
    if (!providerName) continue
    for (const model of splitModels(channel.model)) {
      const price = prices.get(model)
      if (!price) continue
      requests.push({
        path: '/api/v1/models/pricing', method: 'POST', kind: 'price', provider: providerName,
        body: { model, input_price_micros: price.input, output_price_micros: price.output },
      })
    }
  }
  return requests
}

// api_keys → client key create requests. Empty supported-models means all.
function mapKeys(backup) {
  const requests = []
  for (const key of backup.api_keys || []) {
    const models = splitModels(key.supported_models)
    requests.push({
      path: '/api/v1/keys', method: 'POST', kind: 'key',
      body: {
        name: String(key.name || `key-${key.id}`),
        models: models.length ? models : ['*'],
        token_limit: Number(key.max_tokens) || 0,
        max_amount_micros: Math.round((Number(key.max_cost) || 0) * 1_000_000),
        rpm: Number(key.rate_limit_rpm) || 0,
        tpm: Number(key.rate_limit_tpm) || 0,
        expires_at: null,
        secret: key.api_key ? String(key.api_key) : null,
      },
      enabled: key.enabled !== false,
    })
  }
  return requests
}

// Full plan: ordered phases. Providers must exist before routes/prices, and
// route candidates / price provider ids are resolved against the live list.
function buildPlan(backup, existing) {
  const { requests: providerRequests, providerMap } = mapProviders(backup, existing.providers, existing.groups)
  const routeRequests = mapRoutes(backup, providerMap, existing.routes)
  const priceRequests = mapPrices(backup, providerMap)
  const keyRequests = mapKeys(backup)
  return {
    providerMap,
    phases: [
      { id: 'providers', requests: providerRequests },
      { id: 'routes', requests: routeRequests },
      { id: 'prices', requests: priceRequests },
      { id: 'keys', requests: keyRequests },
    ],
  }
}
  return { slugify, uniqueSlug, protocolFor, splitModels, priceMicros, strategyFor, mapProviders, mapRoutes, resolveCandidates, mapPrices, mapKeys, buildPlan }
})()
if (typeof window !== 'undefined') window.OctopusMapping = OctopusMapping