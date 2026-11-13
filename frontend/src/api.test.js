import assert from 'node:assert/strict'
import test from 'node:test'
import { apiClient, keyInput, policyInput, mapProvider, mapRequest, mapRoute, providerInput, routeInput } from './api.ts'
import { scopedModels } from './key-models.ts'

const remoteProvider = { id: 7, name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], key_count: 2, enabled: true, group_id: 'official' }
const remoteRoute = { id: 4, slug: 'chat', name: 'Chat route', description: 'Real description', patterns: ['chat-*'], is_default: true, list_members: true, enabled: true, candidates: [[7, 'local-model']] }
const remoteRequest = { id: 'req-real', protocol: 'openai', model: 'chat', status: 'success', input_tokens: 10, output_tokens: 5, cost_micros: 42, latency_ms: 25, created_at: '2026-11-11T10:00:00Z' }

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
  assert.equal(mapRoute(remoteRoute, [mapProvider(remoteProvider)]).primaryModel, 'local-model')
  assert.equal(mapRequest(remoteRequest).tokens, '15')
  assert.equal(mapRequest(remoteRequest).status, '200 OK')
  assert.equal(mapRequest(remoteRequest).cost, '$0.000042')
})

test('serializes editor data for the real management API', () => {
  const provider = mapProvider(remoteProvider)
  const route = mapRoute(remoteRoute, [provider])
  assert.deepEqual(providerInput({ ...provider, keyDrafts: [{ value: 'secret' }] }), { name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], secret: 'secret', group_id: 'official', enabled: true })
  assert.deepEqual(routeInput(route, [provider]), { slug: 'chat', name: 'Chat route', description: 'Real description', patterns: ['chat-*'], is_default: true, list_members: true, enabled: true, candidates: [[7, 'local-model']] })
})

test('uses the first entered secret for discovery and retains additional keys', () => {
  const provider = mapProvider(remoteProvider)
  assert.deepEqual(providerInput(provider, ['first-secret', 'second-secret']), { name: 'Local OpenAI', protocol: 'openai', endpoint: 'http://127.0.0.1:9000/v1', models: ['local-model'], secret: 'first-secret', secrets: ['first-secret', 'second-secret'], group_id: 'official', enabled: true })
})

test('serializes key creation and policy without accepting a client-generated secret', () => {
  const policy = { name: 'CI', customValue: 'sk-client-value', tags: [], maxAmount: 12.5, tokenLimit: 5000, rpm: 60, tpm: 10000, modelQuotas: { chat: { rpm: 10, tpm: 500 } }, ipWhitelist: ['127.0.0.1'], expiresAt: '2026-12-01', models: ['chat'], allowedGroups: ['official'], excludedProviders: ['7'], enabled: true }
  assert.deepEqual(keyInput(policy), { name: 'CI', models: ['chat'], token_limit: 5000, max_amount_micros: 12500000, rpm: 60, tpm: 10000, expires_at: '2026-12-01' })
  assert.deepEqual(policyInput(policy, [mapProvider(remoteProvider)]), { ip_whitelist: ['127.0.0.1'], model_quotas: { chat: { rpm: 10, tpm: 500 } }, allowed_groups: ['official'], excluded_providers: [7] })
})

test('preserves the selected client key expiration for the gateway', () => {
  const policy = { name: 'timed', customValue: '', tags: [], maxAmount: null, tokenLimit: null, rpm: 0, tpm: 0, modelQuotas: {}, ipWhitelist: [], expiresAt: '2027-12-31T18:30', models: ['chat'], allowedGroups: [], excludedProviders: [], enabled: true }
  assert.equal(keyInput(policy).expires_at, '2027-12-31T18:30')
})

test('loads workspace only through real API and surfaces failures', async () => {
  const calls = []
  const client = apiClient(async (url, options) => { calls.push({ url, options }); return { ok: true, json: async () => [] } })
  await client('/api/v1/providers')
  assert.equal(calls[0].url, '/api/v1/providers')
  assert.equal(calls[0].options.credentials, 'same-origin')
  await assert.rejects(apiClient(async () => ({ ok: false, status: 503, json: async () => ({ error: { message: 'offline' } }) }))('/api/v1/providers'), /offline/)
})
