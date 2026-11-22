import assert from 'node:assert/strict'
import test from 'node:test'

import {
  filterFamilies, formatContext, formatPrice, formatReferencePrice, globMatch,
  groupFamilies, mapCatalog, modelKind, modelRoutable, paginateFamilies, routeForModel,
} from './model-plaza.ts'

const route = (overrides = {}) => ({
  id: 1, name: 'smart', slug: 'smart-x', description: '', match: '*', primary: 'alpha', primaryModel: 'gpt-x',
  fallback: '', fallbackModel: '', strategy: 'Failover', active: true, requests: '0',
  patterns: ['smart-x'], listMembers: true, isDefault: false,
  candidates: [{ id: '1-0', provider: 'alpha', model: 'gpt-x', enabled: true, weight: 1 }],
  ...overrides,
})

const channel = (overrides = {}) => ({
  provider_id: 1, provider: 'alpha', group_id: 'official', protocol: 'openai', enabled: true,
  key_count: 2, input_price_micros: 1_000_000, output_price_micros: 2_000_000,
  routes: [{ slug: 'smart-x', name: 'Smart X' }],
  ...overrides,
})

const meta = (overrides = {}) => ({
  source_id: 'openai/gpt-x', base_model: 'gpt-x', display_name: 'GPT-X', description: 'a model',
  context_length: 128000, input_modalities: ['text', 'image'], output_modalities: ['text'],
  capabilities: ['tools', 'reasoning', 'vision'], input_price_nanos: 1_000, output_price_nanos: 2_000,
  ...overrides,
})

const catalog = [
  { model: 'gpt-x', base_model: 'gpt-x', metadata: meta(), channels: [channel(), channel({ provider_id: 2, provider: 'beta', group_id: 'free', enabled: false, key_count: 0, input_price_micros: null, output_price_micros: null, routes: [] })] },
  { model: 'gpt-x-2024-08-06', base_model: 'gpt-x', metadata: meta({ source_id: 'openai/gpt-x-2024-08-06', display_name: 'GPT-X (dated)' }), channels: [channel({ routes: [] })] },
  { model: 'text-embedding-3-small', base_model: 'text-embedding-3-small', metadata: null, channels: [channel({ key_count: 1, input_price_micros: 20_000, output_price_micros: null, routes: [] })] },
]

test('mapCatalog converts snake_case records including metadata', () => {
  const models = mapCatalog(catalog)
  assert.equal(models[0].baseModel, 'gpt-x')
  assert.equal(models[0].metadata.displayName, 'GPT-X')
  assert.equal(models[0].metadata.inputPriceNanos, 1_000)
  assert.equal(models[0].channels[0].inputPriceMicros, 1_000_000)
  assert.equal(models[0].channels[1].inputPriceMicros, null)
  assert.equal(models[2].metadata, null)
})

test('mapCatalog normalizes owning-route strategies for plaza chips', () => {
  const models = mapCatalog([catalog[0]])
  // Fixture routes omit strategy (legacy wire shape) → priority fallback.
  assert.deepEqual(models[0].channels[0].routes, [{ slug: 'smart-x', name: 'Smart X', strategy: 'priority' }])
  const weighted = mapCatalog([{ model: 'gpt-x', base_model: 'gpt-x', metadata: null, channels: [channel({ routes: [{ slug: 'smart-x', name: 'Smart X', strategy: 'weighted' }] })] }])
  assert.equal(weighted[0].channels[0].routes[0].strategy, 'weighted')
})

test('groupFamilies collapses variants of one base model', () => {
  const families = groupFamilies(mapCatalog(catalog))
  assert.equal(families.length, 2)
  const gpt = families.find((f) => f.base === 'gpt-x')
  assert.equal(gpt.variants.length, 2)
  assert.equal(gpt.variants[0].model, 'gpt-x')
  assert.equal(gpt.displayName, 'GPT-X')
  assert.equal(gpt.channels.length, 3)
  const embed = families.find((f) => f.base === 'text-embedding-3-small')
  assert.equal(embed.metadata, null)
  assert.equal(embed.displayName, 'text-embedding-3-small')
})

test('globMatch mirrors the gateway wildcard semantics', () => {
  assert.equal(globMatch('*', 'anything'), true)
  assert.equal(globMatch('gpt-*', 'gpt-4o'), true)
  assert.equal(globMatch('gpt-*', 'claude-3'), false)
  assert.equal(globMatch('*-free', 'model-free'), true)
  assert.equal(globMatch('a*b*c', 'axxbyyc'), true)
  assert.equal(globMatch('a*b*c', 'axxbyy'), false)
  assert.equal(globMatch('exact', 'exact'), true)
  assert.equal(globMatch('exact', 'exactly'), false)
  assert.equal(globMatch('', ''), true)
  assert.equal(globMatch('', 'x'), false)
})

test('routeForModel prefers slug, then exact candidate, then pattern, then default', () => {
  const routes = [
    route({ slug: 'other', patterns: ['gpt-*'], candidates: [] }),
    route({ slug: 'default', isDefault: true, patterns: [], candidates: [] }),
    route(),
  ]
  assert.equal(routeForModel(routes, 'smart-x').slug, 'smart-x')
  assert.equal(routeForModel(routes, 'gpt-x').slug, 'smart-x')
  assert.equal(routeForModel(routes, 'gpt-z').slug, 'other')
  assert.equal(routeForModel(routes, 'zzz').slug, 'default')
  assert.equal(routeForModel([route({ active: false })], 'gpt-x'), null)
})

test('modelRoutable is false without any enabled matching route', () => {
  const models = mapCatalog(catalog)
  assert.equal(modelRoutable([route()], models[0]), true)
  assert.equal(modelRoutable([], models[0]), false)
})

test('modelKind classifies by naming convention', () => {
  assert.equal(modelKind('gpt-4o'), 'chat')
  assert.equal(modelKind('claude-3-5-sonnet'), 'chat')
  assert.equal(modelKind('bge-reranker-v2'), 'rerank')
  assert.equal(modelKind('text-embedding-3-small'), 'embedding')
  assert.equal(modelKind('omni-moderation-latest'), 'moderation')
  assert.equal(modelKind('suno_music'), 'music')
  assert.equal(modelKind('dall-e-3'), 'image')
  assert.equal(modelKind('gpt-image-1'), 'image')
  assert.equal(modelKind('whisper-1'), 'audio')
  assert.equal(modelKind('sora-2'), 'video')
  assert.equal(modelKind('kimi-k3'), 'chat')
})

test('filterFamilies combines query, kind, group, provider, routable and availability', () => {
  const families = groupFamilies(mapCatalog(catalog))
  const routes = [route()]
  const base = { query: '', kind: 'all', groupId: 'all', providerId: 'all', routableOnly: false, availableOnly: false }
  assert.equal(filterFamilies(families, routes, base).length, 2)
  assert.equal(filterFamilies(families, routes, { ...base, query: 'EMBED' })[0].base, 'text-embedding-3-small')
  assert.equal(filterFamilies(families, routes, { ...base, query: 'beta' })[0].base, 'gpt-x')
  assert.equal(filterFamilies(families, routes, { ...base, kind: 'chat' }).length, 1)
  assert.equal(filterFamilies(families, routes, { ...base, groupId: 'free' })[0].base, 'gpt-x')
  assert.equal(filterFamilies(families, routes, { ...base, providerId: '2' })[0].base, 'gpt-x')
  assert.equal(filterFamilies(families, routes, { ...base, routableOnly: true }).length, 1)
  assert.equal(filterFamilies(families, routes, { ...base, providerId: '2', availableOnly: true }).length, 0)
  assert.equal(filterFamilies(families, routes, { ...base, availableOnly: true }).length, 2)
  // query matches the family display name even when no variant id contains it
  assert.equal(filterFamilies(families, routes, { ...base, query: 'GPT-X' })[0].base, 'gpt-x')
})

test('large model catalogs render in bounded pages without losing records', () => {
  const families = Array.from({ length: 300 }, (_, index) => ({ base: `model-${index}` }))
  const first = paginateFamilies(families, 1, 48)
  assert.equal(first.total, 300)
  assert.equal(first.pages, 7)
  assert.equal(first.page, 1)
  assert.equal(first.items.length, 48)
  assert.equal(first.items[0].base, 'model-0')
  assert.equal(paginateFamilies(families, 7, 48).items.at(-1).base, 'model-299')
  assert.equal(paginateFamilies(families, 99, 48).page, 7)
  assert.equal(paginateFamilies(families, -2, 48).page, 1)
  assert.equal(paginateFamilies([], 3, 48).pages, 1)
  assert.equal(paginateFamilies([], 3, 48).items.length, 0)
})

test('formatPrice renders per-million USD prices', () => {
  assert.equal(formatPrice(null), '—')
  assert.equal(formatPrice(1_000_000), '$1.00')
  assert.equal(formatPrice(2_500_000), '$2.50')
  assert.equal(formatPrice(20_000), '$0.02')
  assert.equal(formatPrice(150_000_000), '$150')
  assert.equal(formatPrice(2_000), '$0.002')
})

test('formatReferencePrice renders nanos per token as per-million USD', () => {
  assert.equal(formatReferencePrice(null), '—')
  assert.equal(formatReferencePrice(1_000), '$1.00')
  assert.equal(formatReferencePrice(2_500), '$2.50')
  assert.equal(formatReferencePrice(20), '$0.02')
  assert.equal(formatReferencePrice(150_000), '$150')
  assert.equal(formatReferencePrice(2), '$0.002')
})

test('formatContext renders token windows compactly', () => {
  assert.equal(formatContext(0), '—')
  assert.equal(formatContext(8192), '8K')
  assert.equal(formatContext(128000), '128K')
  assert.equal(formatContext(1_000_000), '1M')
  assert.equal(formatContext(2_500_000), '2.5M')
})
