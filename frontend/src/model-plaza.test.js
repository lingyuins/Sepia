import assert from 'node:assert/strict'
import test from 'node:test'

import { filterPlaza, formatPrice, globMatch, mapCatalog, modelKind, modelRoutable, routeForModel } from './model-plaza.ts'

const route = (overrides = {}) => ({
  id: 1, name: 'smart', slug: 'smart-x', description: '', match: '*', primary: 'alpha', primaryModel: 'gpt-x',
  fallback: '', fallbackModel: '', strategy: 'Failover', active: true, requests: '0',
  patterns: ['smart-x'], listMembers: true, isDefault: false,
  candidates: [{ id: '1-0', provider: 'alpha', model: 'gpt-x', enabled: true, weight: 1 }],
  ...overrides,
})

const catalog = [
  {
    model: 'gpt-x',
    channels: [
      { provider_id: 1, provider: 'alpha', group_id: 'official', protocol: 'openai', enabled: true, key_count: 2, input_price_micros: 1_000_000, output_price_micros: 2_000_000, routes: [{ slug: 'smart-x', name: 'Smart X' }] },
      { provider_id: 2, provider: 'beta', group_id: 'free', protocol: 'openai', enabled: false, key_count: 0, input_price_micros: null, output_price_micros: null, routes: [] },
    ],
  },
  {
    model: 'text-embedding-3-small',
    channels: [
      { provider_id: 1, provider: 'alpha', group_id: 'official', protocol: 'openai', enabled: true, key_count: 1, input_price_micros: 20_000, output_price_micros: null, routes: [] },
    ],
  },
]

test('mapCatalog converts snake_case records to plaza channels', () => {
  const models = mapCatalog(catalog)
  assert.equal(models[0].channels[0].providerId, 1)
  assert.equal(models[0].channels[0].inputPriceMicros, 1_000_000)
  assert.equal(models[0].channels[1].inputPriceMicros, null)
  assert.equal(models[1].channels[0].keyCount, 1)
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
  const slugRoute = route({ id: 2, slug: 'gpt-x', patterns: ['never'] })
  const candidateRoute = route()
  const patternRoute = route({ id: 3, slug: 'pattern', patterns: ['gpt-*'], candidates: [] })
  const defaultRoute = route({ id: 4, slug: 'default', patterns: ['nomatch'], isDefault: true, candidates: [] })
  assert.equal(routeForModel([defaultRoute, patternRoute, candidateRoute, slugRoute], 'gpt-x')?.id, 2)
  assert.equal(routeForModel([defaultRoute, patternRoute, candidateRoute], 'gpt-x')?.id, 1)
  assert.equal(routeForModel([defaultRoute, patternRoute], 'gpt-4o')?.id, 3)
  assert.equal(routeForModel([defaultRoute], 'unknown-model')?.id, 4)
  assert.equal(routeForModel([route({ active: false })], 'gpt-x'), undefined)
})

test('modelRoutable is false without any enabled matching route', () => {
  assert.equal(modelRoutable([], 'gpt-x'), false)
  assert.equal(modelRoutable([route()], 'gpt-x'), true)
})

test('modelKind classifies by naming convention', () => {
  assert.equal(modelKind('gpt-4o'), 'chat')
  assert.equal(modelKind('claude-opus-4'), 'chat')
  assert.equal(modelKind('deepseek-r1'), 'chat')
  assert.equal(modelKind('text-embedding-3-small'), 'embedding')
  assert.equal(modelKind('bge-reranker-v2'), 'rerank')
  assert.equal(modelKind('omni-moderation-latest'), 'moderation')
  assert.equal(modelKind('dall-e-3'), 'image')
  assert.equal(modelKind('flux-1.1-pro'), 'image')
  assert.equal(modelKind('whisper-1'), 'audio')
  assert.equal(modelKind('tts-1-hd'), 'audio')
  assert.equal(modelKind('sora-2'), 'video')
  assert.equal(modelKind('suno_music'), 'music')
  assert.equal(modelKind('mystery-model'), 'other')
})

test('filterPlaza combines query, kind, group, provider, routable and availability', () => {
  const models = mapCatalog(catalog)
  const routes = [route()]
  const base = { query: '', kind: 'all', groupId: 'all', providerId: 'all', routableOnly: false, availableOnly: false }
  assert.equal(filterPlaza(models, routes, base).length, 2)
  assert.equal(filterPlaza(models, routes, { ...base, query: 'EMBED' })[0].model, 'text-embedding-3-small')
  assert.equal(filterPlaza(models, routes, { ...base, query: 'beta' })[0].model, 'gpt-x')
  assert.equal(filterPlaza(models, routes, { ...base, kind: 'chat' }).length, 1)
  assert.equal(filterPlaza(models, routes, { ...base, groupId: 'free' })[0].model, 'gpt-x')
  assert.equal(filterPlaza(models, routes, { ...base, providerId: 2 })[0].model, 'gpt-x')
  assert.equal(filterPlaza(models, routes, { ...base, routableOnly: true }).length, 1)
  // beta-only channel is disabled with zero keys: availability filter drops gpt-x.
  assert.equal(filterPlaza(models, routes, { ...base, providerId: 2, availableOnly: true }).length, 0)
  assert.equal(filterPlaza(models, routes, { ...base, availableOnly: true }).length, 2)
})

test('formatPrice renders per-million USD prices', () => {
  assert.equal(formatPrice(null), '—')
  assert.equal(formatPrice(1_000_000), '$1.00')
  assert.equal(formatPrice(2_500_000), '$2.50')
  assert.equal(formatPrice(20_000), '$0.02')
  assert.equal(formatPrice(150_000_000), '$150')
  assert.equal(formatPrice(2_000), '$0.002')
})