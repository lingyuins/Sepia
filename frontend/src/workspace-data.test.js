import assert from 'node:assert/strict'
import test from 'node:test'
import { defaultSettings } from './settings.ts'
import { emptyAnalytics, loadWorkspaceSnapshot, mergeServerSettings } from './workspace-data.ts'

const provider = {
  id: 7,
  name: 'Alpha',
  protocol: 'openai',
  endpoint: 'http://alpha.test/v1',
  models: ['model-a'],
  key_count: 1,
  enabled: true,
  group_id: 'official',
  mode: 'single',
  keys: [{ id: 11, models: null, endpoint_id: null }],
  endpoints: [],
}
const route = {
  id: 4,
  slug: 'chat',
  name: 'Chat',
  description: '',
  patterns: ['model-a'],
  is_default: false,
  list_members: false,
  enabled: true,
  candidates: [[7, 'model-a']],
  strategy: 'priority',
  weights: [1],
}
const analytics = {
  requests: 10,
  successful: 9,
  input_tokens: 100,
  output_tokens: 50,
  cost_micros: 42,
  by_provider: [{ provider_id: 7, requests: 10, successful: 9, latency_ms: 25 }],
}

function fixtureFetch(calls, overrides = {}) {
  return async (url) => {
    calls.push(url)
    if (url === '/api/v1/providers') return Response.json([overrides.provider ?? provider])
    if (url === '/api/v1/routes') return Response.json([route])
    if (url.startsWith('/api/v1/analytics')) return Response.json(overrides.analytics ?? analytics)
    if (url === '/api/v1/settings') return Response.json(overrides.settings ?? { workspace_name: 'Server workspace', max_attempts: 5 })
    if (url === '/api/v1/version') return Response.json({ version: '0.1.3' })
    if (url === '/api/v1/requests') return Response.json([])
    if (url === '/api/v1/keys') return Response.json([])
    if (url === '/api/v1/groups') return Response.json([])
    if (url === '/api/v1/plugins') return Response.json([])
    throw new Error(`Unexpected request: ${url}`)
  }
}

const current = {
  providers: [],
  routes: [],
  requests: [],
  analytics: emptyAnalytics,
  clientKeys: [],
  channelGroups: [],
  plugins: [],
  version: '',
  settings: { ...defaultSettings, language: '简体中文', theme: 'Light' },
}

test('scoped refresh loads only requested domains and preserves other references', async () => {
  const calls = []
  const seeded = await loadWorkspaceSnapshot(fixtureFetch(calls), current, ['providers', 'routes', 'analytics', 'settings'], '7 days', true)
  calls.length = 0
  const next = await loadWorkspaceSnapshot(fixtureFetch(calls, { provider: { ...provider, name: 'Renamed' } }), seeded, ['providers', 'routes'], '7 days', false)
  assert.deepEqual(calls.sort(), ['/api/v1/providers', '/api/v1/routes'])
  assert.equal(next.providers[0].name, 'Renamed')
  assert.equal(next.providers[0].success, '90.00%')
  assert.equal(next.routes[0].candidates[0].provider, 'Renamed')
  assert.equal(next.requests, seeded.requests)
  assert.equal(next.analytics, seeded.analytics)
  assert.equal(next.settings, seeded.settings)
  assert.equal(next.version, seeded.version)
})

test('analytics refresh updates provider stats without refetching configuration', async () => {
  const calls = []
  const seeded = await loadWorkspaceSnapshot(fixtureFetch(calls), current, ['providers', 'routes', 'analytics'], '30 days', false)
  calls.length = 0
  const next = await loadWorkspaceSnapshot(fixtureFetch(calls, { analytics: { ...analytics, by_provider: [{ provider_id: 7, requests: 20, successful: 19, latency_ms: 40 }] } }), seeded, ['analytics'], '24 hours', false)
  assert.deepEqual(calls, ['/api/v1/analytics?period=24%20hours'])
  assert.equal(next.providers[0].latency, 40)
  assert.equal(next.providers[0].success, '95.00%')
  assert.equal(next.routes, seeded.routes)
})

test('server settings merge over browser preferences without replacing local choices', () => {
  const merged = mergeServerSettings(current.settings, {
    workspace_name: 'Gateway',
    gateway_host: 'gateway.example',
    gateway_port: '443',
    max_attempts: 6,
    log_bodies: true,
  })
  assert.equal(merged.workspace, 'Gateway')
  assert.equal(merged.host, 'gateway.example')
  assert.equal(merged.port, '443')
  assert.equal(merged.retries, '6')
  assert.equal(merged.logBodies, true)
  assert.equal(merged.language, '简体中文')
  assert.equal(merged.theme, 'Light')
})

test('overlapping scoped refresh merges against the latest committed snapshot', async () => {
  const seeded = await loadWorkspaceSnapshot(fixtureFetch([]), current, ['providers', 'routes', 'analytics'], '30 days', false)
  let latest = seeded
  let releaseScoped
  const scopedGate = new Promise((resolve) => { releaseScoped = resolve })
  let scopedStarted
  const scopedStart = new Promise((resolve) => { scopedStarted = resolve })
  const scopedFetch = async (url) => {
    if (url === '/api/v1/providers') {
      scopedStarted()
      await scopedGate
      return Response.json([{ ...provider, name: 'Scoped rename' }])
    }
    return fixtureFetch([])(url)
  }
  const scopedPromise = loadWorkspaceSnapshot(scopedFetch, () => latest, ['providers'], '30 days', false, (next) => { latest = next })
  await scopedStart
  const full = await loadWorkspaceSnapshot(fixtureFetch([], { provider: { ...provider, name: 'Full rename' } }), () => latest, ['providers', 'routes', 'analytics'], '30 days', false, (next) => { latest = next })
  releaseScoped()
  const scoped = await scopedPromise
  assert.equal(scoped.providers[0].name, 'Scoped rename')
  assert.equal(scoped.routes[0].candidates[0].provider, 'Scoped rename')
  assert.equal(scoped.analytics, full.analytics)
})
