import test from 'node:test'
import assert from 'node:assert/strict'
import { aiServiceInput, buildFamilyHints, emptyAiServiceDraft, filterGeneratedRoutes, generatedRouteRule, mapAiPoolService, serviceDraftFrom, validateAiServiceDraft } from './ai-pool.ts'
import { compileNormalizeRules } from './model-normalize.ts'
import { routeInput } from './api.ts'

const providers = [
  { id: 1, name: 'Alpha', enabled: true, modelsList: ['gpt-x', 'gpt-y'] },
  { id: 2, name: 'Beta', enabled: true, modelsList: ['gpt-x'] },
  { id: 3, name: 'Gamma', enabled: false, modelsList: ['gpt-x'] },
]

const draft = (patch = {}) => ({ ...emptyAiServiceDraft, name: 'Planner', endpoint: 'https://ai.example/v1', secret: 'sk-test', model: 'planner-x', ...patch })

test('service draft validation mirrors backend limits', () => {
  assert.equal(validateAiServiceDraft(draft(), false), null)
  assert.deepEqual(validateAiServiceDraft(draft({ name: '  ' }), false), { key: 'aiServiceNameRequired' })
  assert.deepEqual(validateAiServiceDraft(draft({ endpoint: 'ftp://ai.example' }), false), { key: 'aiServiceEndpointInvalid' })
  assert.deepEqual(validateAiServiceDraft(draft({ endpoint: 'not a url' }), false), { key: 'aiServiceEndpointInvalid' })
  assert.deepEqual(validateAiServiceDraft(draft({ secret: '' }), false), { key: 'aiServiceKeyRequired' })
  // Editing keeps the stored key when the secret is blank.
  assert.equal(validateAiServiceDraft(draft({ secret: '' }), true), null)
  assert.deepEqual(validateAiServiceDraft(draft({ model: '' }), false), { key: 'aiServiceModelRequired' })
  for (const timeoutSeconds of ['0', '601', '1.5', 'abc']) assert.deepEqual(validateAiServiceDraft(draft({ timeoutSeconds }), false), { key: 'aiServiceTimeoutInvalid' })
  for (const concurrency of ['0', '17', '2.5']) assert.deepEqual(validateAiServiceDraft(draft({ concurrency }), false), { key: 'aiServiceConcurrencyInvalid' })
  assert.equal(validateAiServiceDraft(draft({ timeoutSeconds: '600', concurrency: '16' }), false), null)
})

test('service input trims and converts to wire fields', () => {
  const input = aiServiceInput(draft({ name: ' Planner ', endpoint: ' https://ai.example/v1 ', secret: ' sk-test ', timeoutSeconds: '90', concurrency: '4' }))
  assert.deepEqual(input, { name: 'Planner', provider_id: null, endpoint: 'https://ai.example/v1', secret: 'sk-test', model: 'planner-x', models: [], timeout_seconds: 90, concurrency: 4, enabled: true })
})

test('service mapping round-trips through the edit draft', () => {
  const service = mapAiPoolService({ id: 7, name: 'Planner', endpoint: 'https://ai.example/v1', model: 'planner-x', models: ['planner-x', 'planner-y'], timeout_seconds: 45, concurrency: 3, enabled: false, created_at: 'x', updated_at: 'y' })
  assert.deepEqual(service, { id: 7, name: 'Planner', providerId: null, endpoint: 'https://ai.example/v1', model: 'planner-x', models: ['planner-x', 'planner-y'], timeoutSeconds: 45, concurrency: 3, enabled: false })
  const edit = serviceDraftFrom(service)
  assert.equal(edit.secret, '')
  assert.equal(edit.timeoutSeconds, '45')
  assert.equal(edit.concurrency, '3')
  assert.equal(validateAiServiceDraft(edit, true), null)
})

const proposal = (patch = {}) => ({ slug: 'gpt-x', name: 'GPT X', description: 'merged', patterns: ['gpt-x'], candidates: [{ provider_id: 1, provider: 'Alpha', model: 'gpt-x' }, { provider_id: 2, provider: 'Beta', model: 'gpt-x' }], ...patch })

test('generated routes drop unknown, disabled or unserved candidates', () => {
  const routes = filterGeneratedRoutes([proposal({ candidates: [{ provider_id: 1, provider: 'Alpha', model: 'gpt-x' }, { provider_id: 3, provider: 'Gamma', model: 'gpt-x' }, { provider_id: 9, provider: 'Ghost', model: 'gpt-x' }, { provider_id: 2, provider: 'Beta', model: 'gpt-y' }] })], providers, [])
  assert.equal(routes.length, 1)
  assert.deepEqual(routes[0].candidates, [{ provider_id: 1, provider: 'Alpha', model: 'gpt-x' }])
})

test('generated routes rewrite stale provider names to the current name', () => {
  const routes = filterGeneratedRoutes([proposal({ candidates: [{ provider_id: 1, provider: 'Alpha Old', model: 'gpt-x' }, { provider_id: 2, provider: 'Beta Renamed', model: 'gpt-x' }] })], providers, [])
  assert.deepEqual(routes[0].candidates.map((candidate) => [candidate.provider_id, candidate.provider]), [[1, 'Alpha'], [2, 'Beta']])
  // routeInput resolves candidates by provider name, so the rename must not break persistence.
  assert.deepEqual(routeInput(generatedRouteRule(routes[0], 0), providers).candidates, [[1, 'gpt-x'], [2, 'gpt-x']])
})

test('generated routes drop empty proposals and duplicate slugs', () => {
  const routes = filterGeneratedRoutes([proposal(), proposal({ name: 'dup' }), proposal({ slug: 'all-dead', candidates: [{ provider_id: 3, provider: 'Gamma', model: 'gpt-x' }] })], providers, ['existing'])
  assert.deepEqual(routes.map((route) => route.slug), ['gpt-x'])
  assert.deepEqual(filterGeneratedRoutes([proposal()], providers, ['gpt-x']), [])
})

test('generated route rule persists through routeInput with ordered candidates', () => {
  const rule = generatedRouteRule(proposal(), 0)
  assert.equal(rule.slug, 'gpt-x')
  assert.equal(rule.match, 'gpt-x')
  assert.equal(rule.candidates[0].provider, 'Alpha')
  assert.equal(rule.candidates[1].provider, 'Beta')
  assert.equal(rule.active, true)
  assert.equal(rule.strategy, 'Failover')
  assert.deepEqual(rule.candidates.map((candidate) => [candidate.provider, candidate.model]), [['Alpha', 'gpt-x'], ['Beta', 'gpt-x']])
  const wire = routeInput(rule, providers)
  assert.deepEqual(wire.candidates, [[1, 'gpt-x'], [2, 'gpt-x']])
  assert.deepEqual(wire.patterns, ['gpt-x'])
  assert.equal(wire.enabled, true)
  assert.equal(wire.is_default, false)
  assert.equal(wire.list_members, false)
})

test('generated route rule defaults patterns to the slug', () => {
  const rule = generatedRouteRule(proposal({ patterns: [] }), 1)
  assert.deepEqual(rule.patterns, ['gpt-x'])
  assert.equal(rule.match, 'gpt-x')
})

test('family hints normalize variant ids and skip unavailable providers', () => {
  const rules = compileNormalizeRules()
  const hints = buildFamilyHints([
    { id: 1, enabled: true, modelsList: ['kimi-3.1-pro', 'auto/kimi-k3.1-pro'], keyCount: 1 },
    { id: 2, enabled: true, modelsList: ['moonshotai/kimi-k3.1-pro:free'], keyCount: 2 },
    { id: 3, enabled: false, modelsList: ['kimi-k3.1-pro'], keyCount: 1 },
    { id: 4, enabled: true, modelsList: ['kimi-k3.1-pro'], keyCount: 0 },
  ], rules)
  assert.deepEqual(hints, [
    { provider_id: 1, model: 'kimi-3.1-pro', family: 'kimi-3.1-pro' },
    { provider_id: 1, model: 'auto/kimi-k3.1-pro', family: 'kimi-k3.1-pro' },
    { provider_id: 2, model: 'moonshotai/kimi-k3.1-pro:free', family: 'kimi-k3.1-pro' },
  ])
})

test('family hint cap counts Unicode code points, matching the backend', () => {
  const rules = compileNormalizeRules()
  const astral = '😀'.repeat(130)
  const hints = buildFamilyHints([
    { id: 1, enabled: true, modelsList: [astral], keyCount: 1 },
  ], rules)
  assert.equal(hints.length, 1)
  assert.equal([...hints[0].family].length, 128)
  assert.equal(hints[0].family, '😀'.repeat(128))
})
