import assert from 'node:assert/strict'
import test from 'node:test'
import { ApiError, apiClient, keyInput, policyInput, mapProvider, mapRequest, mapRoute, providerInput, routeInput } from './api.ts'
import { scopedModels } from './key-models.ts'

const remoteProvider = { id: 7, name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], key_count: 2, enabled: true, group_id: 'official' }
const remoteRoute = { id: 4, slug: 'chat', name: 'Chat route', description: 'Real description', patterns: ['chat-*'], is_default: true, list_members: true, enabled: true, candidates: [[7, 'local-model']] }
const thirdProvider = { ...remoteProvider, id: 8, name: 'Third provider' }
const threeCandidateRoute = { ...remoteRoute, candidates: [[7, 'local-model'], [8, 'local-model'], [7, 'backup-model']] }
const remoteRequest = { id: 'req-real', protocol: 'openai', model: 'local-model', route: 'chat', status: 'success', input_tokens: 10, output_tokens: 5, cost_micros: 42, latency_ms: 25, created_at: '2026-11-11T10:00:00Z' }

test('offers smart route groups separately from provider models', () => {
  const models = scopedModels([mapProvider(remoteProvider)], [mapRoute(remoteRoute, [mapProvider(remoteProvider)])])
  assert.equal(models.length, 2)
  assert.deepEqual(models.filter((model) => model.route), [{ value: 'chat', label: 'Chat route', detail: 'chat', route: true }])
  assert.equal(models.filter((model) => !model.route).length, 1)
})

test('maps real backend records to existing UI shapes', () => {
  assert.equal(mapProvider(remoteProvider).id, 7)
  assert.equal(mapProvider(remoteProvider).keyCount, 2)
  assert.equal(mapProvider(remoteProvider).models, 1)
  assert.equal(mapRoute(remoteRoute, [mapProvider(remoteProvider)]).slug, 'chat')
  assert.equal(mapRoute(remoteRoute, [mapProvider(remoteProvider)]).name, 'Chat route')
  assert.equal(mapRoute(remoteRoute, [mapProvider(remoteProvider)]).description, 'Real description')
  assert.equal(mapRoute(remoteRoute, [mapProvider(remoteProvider)]).candidates[0].model, 'local-model')
  assert.equal(mapRequest(remoteRequest).tokens, '15')
  assert.equal(mapRequest(remoteRequest).status, '200 OK')
  assert.equal(mapRequest(remoteRequest).cost, '$0.000042')
  assert.equal(mapRequest(remoteRequest).model, 'local-model')
  assert.equal(mapRequest(remoteRequest).route, 'chat')
})

test('serializes editor data for the real management API', () => {
  const provider = mapProvider(remoteProvider)
  const route = mapRoute(remoteRoute, [provider])
  assert.deepEqual(providerInput(provider, [{ value: 'secret', models: null }]), { name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], secret: 'secret', secrets: ['secret'], key_models: [], group_id: 'official', enabled: true, mode: 'single' })
  assert.deepEqual(routeInput(route, [provider]), { slug: 'chat', name: 'Chat route', description: 'Real description', patterns: ['chat-*'], is_default: true, list_members: true, enabled: true, candidates: [[7, 'local-model']], strategy: 'priority', weights: [1] })
  // Unspecified listing defaults to group-only so member models stay hidden.
  assert.equal(routeInput({ ...route, listMembers: undefined }, [provider]).list_members, false)
})

test('round-trips strategy and per-candidate weights through the route API shapes', () => {
  const provider = mapProvider(remoteProvider)
  const remote = { ...remoteRoute, candidates: [[7, 'local-model'], [7, 'local-model']], strategy: 'weighted', weights: [70, 30] }
  const route = mapRoute(remote, [provider])
  assert.equal(route.strategy, 'weighted')
  assert.deepEqual(route.candidates.map((candidate) => candidate.weight), [70, 30])
  const input = routeInput(route, [provider])
  assert.equal(input.strategy, 'weighted')
  assert.deepEqual(input.weights, [70, 30])
  // Legacy records without strategy/weights keep priority and unit weights.
  const legacy = mapRoute(remoteRoute, [provider])
  assert.equal(legacy.strategy, 'priority')
  assert.deepEqual(routeInput(legacy, [provider]).weights, [1])
})

test('routes retain every candidate without primary/fallback copies', () => {
  const route = mapRoute(threeCandidateRoute, [mapProvider(remoteProvider), mapProvider(thirdProvider)])
  assert.deepEqual(route.candidates.map(({ provider, model }) => ({ provider, model })), [
    { provider: 'Local OpenAI', model: 'local-model' },
    { provider: 'Third provider', model: 'local-model' },
    { provider: 'Local OpenAI', model: 'backup-model' },
  ])
  assert.equal('primary' in route, false)
  assert.equal('fallback' in route, false)
})

test('uses the first entered secret for discovery and retains additional keys', () => {
  const provider = mapProvider(remoteProvider)
  assert.deepEqual(providerInput(provider, [{ value: 'first-secret', models: ['claude-a'] }, { value: 'second-secret', models: null }]), { name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], secret: 'first-secret', secrets: ['first-secret', 'second-secret'], key_models: [{ secret: 'first-secret', models: ['claude-a'] }], group_id: 'official', enabled: true, mode: 'single' })
})

test('maps unified channel addresses and serializes per-address keys', () => {
  const remote = { ...remoteProvider, mode: 'unified', keys: [{ id: 11, models: null, endpoint_id: 21 }, { id: 12, models: ['m-b'], endpoint_id: 22 }], endpoints: [{ id: 21, endpoint: 'http://a.test/v1', keys: [{ id: 11, models: null }] }, { id: 22, endpoint: 'http://b.test/v1', keys: [{ id: 12, models: ['m-b'] }] }] }
  const provider = mapProvider(remote)
  assert.equal(provider.mode, 'unified')
  assert.deepEqual(provider.endpoints.map((entry) => entry.endpoint), ['http://a.test/v1', 'http://b.test/v1'])
  assert.deepEqual(provider.keyScopes.map((key) => key.endpointId), [21, 22])
  const input = providerInput(provider, [
    { value: '', models: null, endpointId: 21 },
    { value: 'new-a', models: null, endpointId: 21 },
    { value: 'new-b', models: ['m-b'], endpointId: 22 },
  ])
  assert.equal(input.endpoint, 'http://a.test/v1')
  assert.equal(input.secret, 'new-a')
  assert.deepEqual(input.endpoints, [
    { endpoint: 'http://a.test/v1', secrets: ['new-a'], key_models: [] },
    { endpoint: 'http://b.test/v1', secrets: ['new-b'], key_models: [{ secret: 'new-b', models: ['m-b'] }] },
  ])
})

test('serializes key creation and policy without accepting a client-generated secret', () => {
  const policy = { name: 'CI', customValue: 'sk-client-value', maxAmount: 12.5, tokenLimit: 5000, rpm: 60, tpm: 10000, modelQuotas: { chat: { rpm: 10, tpm: 500 } }, ipWhitelist: ['127.0.0.1'], expiresAt: '2026-12-01', models: ['chat'], allowedGroups: ['official'], modelGroups: [3], excludedProviders: ['7'], enabled: true }
  assert.deepEqual(keyInput(policy), { name: 'CI', models: ['chat'], token_limit: 5000, max_amount_micros: 12500000, rpm: 60, tpm: 10000, expires_at: '2026-12-01' })
  assert.deepEqual(policyInput(policy, [mapProvider(remoteProvider)]), { ip_whitelist: ['127.0.0.1'], model_quotas: { chat: { rpm: 10, tpm: 500 } }, allowed_groups: ['official'], model_groups: [3], excluded_providers: [7] })
})

test('preserves the selected client key expiration for the gateway', () => {
  const policy = { name: 'timed', customValue: '', maxAmount: null, tokenLimit: null, rpm: 0, tpm: 0, modelQuotas: {}, ipWhitelist: [], expiresAt: '2027-12-31T18:30', models: ['chat'], allowedGroups: [], excludedProviders: [], enabled: true }
  assert.equal(keyInput(policy).expires_at, '2027-12-31T18:30')
})

test('loads workspace only through real API and surfaces failures', async () => {
  const calls = []
  const client = apiClient(async (url, options) => { calls.push({ url, options }); return Response.json([]) })
  await client('/api/v1/providers')
  assert.equal(calls[0].url, '/api/v1/providers')
  assert.equal(calls[0].options.credentials, 'same-origin')
  await assert.rejects(apiClient(async () => Response.json({ error: { message: 'offline' } }, { status: 503 }))('/api/v1/providers'), (error) => {
    assert.ok(error instanceof ApiError)
    assert.equal(error.status, 503)
    assert.equal(error.message, 'offline')
    return true
  })
})

test('malformed JSON responses fail instead of becoming empty successful records', async () => {
  const client = apiClient(async () => new Response('<html>Bad Gateway</html>', {
    status: 200,
    headers: { 'content-type': 'application/json' },
  }))
  await assert.rejects(client('/api/v1/models/catalog'), /invalid JSON/)
})

test('empty and plain-text gateway errors remain readable', async () => {
  const noContent = apiClient(async () => new Response(null, { status: 204 }))
  assert.deepEqual(await noContent('/api/v1/requests', { method: 'DELETE' }), {})
  const badGateway = apiClient(async () => new Response('<html>Bad Gateway</html>', {
    status: 502,
    headers: { 'content-type': 'text/html' },
  }))
  await assert.rejects(badGateway('/api/v1/models/catalog'), (error) => {
    assert.equal(error.status, 502)
    assert.equal(error.message, 'Request failed (502)')
    return true
  })
})
