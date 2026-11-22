import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const source = readFileSync(join(here, 'ui', 'mapping.js'), 'utf8')
const M = new Function(`${source}; return OctopusMapping`)()

const backup = {
  channel_groups: [{ id: 3, name: 'Paid' }],
  channels: [
    { id: 8, name: 'Alpha', group_id: 3, type: 1, enabled: true, base_urls: [{ url: 'https://a.example/v1' }], model: 'm1,m2' },
    { id: 9, name: 'Beta', group_id: 3, type: 0, enabled: false, base_urls: [{ url: 'https://b.example/v1' }, { url: 'https://b2.example/v1' }], model: 'm2,m3' },
    { id: 10, name: 'NoKeys', group_id: 3, type: 0, enabled: true, base_urls: [{ url: 'https://c.example/v1' }], model: 'm4' },
  ],
  channel_keys: [
    { id: 1, channel_id: 8, enabled: true, channel_key: 'sk-a', priority: 0 },
    { id: 2, channel_id: 9, enabled: true, channel_key: 'sk-b', priority: 1 },
    { id: 3, channel_id: 9, enabled: false, channel_key: 'sk-disabled', priority: 0 },
  ],
  groups: [
    { id: 100, name: 'Chat Group', mode: 4 },
    { id: 101, name: 'Empty Group', mode: 3 },
  ],
  group_items: [
    { id: 1, group_id: 100, channel_id: 8, model_name: 'm1', priority: 2, weight: 5 },
    { id: 2, group_id: 100, channel_id: 9, model_name: 'm2', priority: 1, weight: 9 },
    { id: 3, group_id: 101, channel_id: 10, model_name: 'm4', priority: 1, weight: 1 },
  ],
  llm_infos: [
    { name: 'm1', input: 0.14, output: 0.28 },
    { name: 'm2', input: 0, output: 0 },
  ],
  api_keys: [
    { id: 6, name: 'Phone', api_key: 'sk-x', enabled: true, supported_models: 'm1,m2', max_cost: 5, rate_limit_rpm: 10 },
    { id: 7, name: 'All', api_key: 'sk-y', enabled: false },
  ],
}

test('slugify and uniqueSlug', () => {
  assert.equal(M.slugify('Chat Group'), 'chat-group')
  assert.equal(M.slugify('额度/渠道监控'), 'imported')
  const used = new Set(['chat-group'])
  assert.equal(M.uniqueSlug('Chat Group', used), 'chat-group-2')
})

test('protocolFor and strategyFor', () => {
  assert.equal(M.protocolFor(1), 'openai-responses')
  assert.equal(M.protocolFor(0), 'openai')
  assert.equal(M.strategyFor(4), 'weighted')
  assert.equal(M.strategyFor(1), 'round-robin')
  assert.equal(M.strategyFor(5), 'priority')
})

test('priceMicros converts $/M to integer micros', () => {
  assert.equal(M.priceMicros(0.14), 140000)
  assert.equal(M.priceMicros(0), 0)
  assert.equal(M.priceMicros(-1), 0)
  assert.equal(M.priceMicros(0.0000001), 1)
})

test('mapProviders skips keyless channels and builds unified endpoints', () => {
  const { requests, providerMap } = M.mapProviders(backup, [], [])
  const providers = requests.filter((request) => request.kind === 'provider')
  assert.equal(providers.length, 2) // NoKeys skipped
  assert.equal(providerMap.has(10), false)
  const alpha = providers.find((request) => request.name === 'Alpha')
  assert.equal(alpha.body.protocol, 'openai-responses')
  assert.equal(alpha.body.mode, 'single')
  assert.equal(alpha.body.secret, 'sk-a')
  assert.equal(alpha.body.group_id, 'octopus-3')
  const beta = providers.find((request) => request.name === 'Beta')
  assert.equal(beta.body.mode, 'unified')
  assert.equal(beta.body.enabled, false)
  assert.equal(beta.body.endpoints.length, 2)
  assert.deepEqual(beta.body.endpoints[0].secrets, ['sk-b']) // disabled key excluded
  const groupRequest = requests.find((request) => request.kind === 'group')
  assert.equal(groupRequest.body.id, 'octopus-3')
})

test('mapProviders dedupes names against existing providers', () => {
  const { requests } = M.mapProviders(backup, [{ name: 'Alpha' }], [])
  const alpha = requests.find((request) => request.kind === 'provider' && request.name.startsWith('Alpha'))
  assert.equal(alpha.name, 'Alpha (8)')
})

test('mapRoutes orders candidates by priority and maps weighted strategy', () => {
  const { providerMap } = M.mapProviders(backup, [], [])
  const routes = M.mapRoutes(backup, providerMap, [])
  assert.equal(routes.length, 1) // Empty Group has no resolvable candidates
  const route = routes[0]
  assert.equal(route.slug, 'chat-group')
  assert.equal(route.body.strategy, 'weighted')
  assert.deepEqual(route.body.candidates, [
    { provider: 'Beta', model: 'm2' }, // priority 1 first
    { provider: 'Alpha', model: 'm1' },
  ])
  assert.deepEqual(route.body.weights, [9, 5])
})

test('resolveCandidates maps names to ids and rejects missing providers', () => {
  const { providerMap } = M.mapProviders(backup, [], [])
  const route = M.mapRoutes(backup, providerMap, [])[0]
  const resolved = M.resolveCandidates(route, [{ name: 'Beta', id: 2 }, { name: 'Alpha', id: 1 }])
  assert.deepEqual(resolved.body.candidates, [[2, 'm2'], [1, 'm1']])
  assert.equal(M.resolveCandidates(route, [{ name: 'Alpha', id: 1 }]), null)
})

test('mapPrices only emits priced models for mapped providers', () => {
  const { providerMap } = M.mapProviders(backup, [], [])
  const prices = M.mapPrices(backup, providerMap)
  assert.equal(prices.length, 1) // m2 unpriced, m4 provider skipped
  assert.equal(prices[0].provider, 'Alpha')
  assert.equal(prices[0].body.model, 'm1')
  assert.equal(prices[0].body.input_price_micros, 140000)
  assert.equal(prices[0].body.output_price_micros, 280000)
})

test('mapKeys defaults to wildcard scope and preserves enabled flag', () => {
  const keys = M.mapKeys(backup)
  assert.equal(keys.length, 2)
  assert.deepEqual(keys[0].body.models, ['m1', 'm2'])
  assert.equal(keys[0].body.max_amount_micros, 5_000_000)
  assert.equal(keys[0].body.rpm, 10)
  assert.equal(keys[0].body.secret, 'sk-x')
  assert.deepEqual(keys[1].body.models, ['*'])
  assert.equal(keys[1].enabled, false)
  assert.equal(keys[1].body.secret, 'sk-y')
})

test('buildPlan orders phases providers→routes→prices→keys', () => {
  const plan = M.buildPlan(backup, { providers: [], routes: [], groups: [] })
  assert.deepEqual(plan.phases.map((phase) => phase.id), ['providers', 'routes', 'prices', 'keys'])
})