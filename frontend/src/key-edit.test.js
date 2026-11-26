import assert from 'node:assert/strict'
import test from 'node:test'

import { keyEditDraft, keyInput, policyInput } from './api.ts'
import { parseKeyPolicy } from './key-policy.ts'

const stored = {
  id: 7,
  name: 'prod-bot',
  models: ['smart-route', 'gpt-4o', 'legacy-model'],
  token_limit: 1000000,
  max_amount_micros: 2500000,
  rpm: 60,
  tpm: 1000,
  expires_at: '2099-01-01T00:00:00.000Z',
  enabled: false,
  model_groups: [3],
  ip_whitelist: ['10.0.0.1', '192.168.0.0/16'],
  model_quotas: { 'gpt-4o': { rpm: 5, tpm: 50000 } },
  allowed_groups: ['official'],
  excluded_providers: [2],
}
const providers = [{ id: 2, name: 'Upstream B' }]

test('splits stored scope back into route and provider selections', () => {
  const draft = keyEditDraft(stored, ['smart-route', 'other-route'], new Map([[2, 'Upstream B']]))
  assert.deepEqual(draft.routeModels, ['smart-route'])
  // Unknown entries stay in providerModels so saving never drops stored grants.
  assert.deepEqual(draft.providerModels, ['gpt-4o', 'legacy-model'])
  assert.deepEqual(draft.modelGroups, [3])
  assert.deepEqual(draft.allowedGroups, ['official'])
  assert.deepEqual(draft.excludedProviders, ['Upstream B'])
  assert.equal(draft.enabled, false)
  assert.equal(draft.customValue, '')
})

test('restores limits, quotas, whitelist and minute-precision expiry', () => {
  const draft = keyEditDraft(stored, ['smart-route'], new Map())
  assert.equal(draft.maxAmount, '2.5')
  assert.equal(draft.tokenLimit, '1000000')
  assert.equal(draft.rpm, '60')
  assert.equal(draft.tpm, '1000')
  assert.deepEqual(JSON.parse(draft.modelQuotas), { 'gpt-4o': { rpm: 5, tpm: 50000 } })
  assert.equal(draft.ipWhitelist, '10.0.0.1, 192.168.0.0/16')
  assert.equal(draft.expiresAt, '2099-01-01T00:00')
})

test('an edited key round-trips to the same wire metadata', () => {
  const draft = keyEditDraft(stored, ['smart-route'], new Map([[2, 'Upstream B']]))
  const policy = parseKeyPolicy(draft)
  const wire = keyInput(policy)
  assert.equal(wire.name, stored.name)
  assert.deepEqual(wire.models, stored.models)
  assert.equal(wire.token_limit, stored.token_limit)
  assert.equal(wire.max_amount_micros, stored.max_amount_micros)
  assert.equal(wire.rpm, stored.rpm)
  assert.equal(wire.tpm, stored.tpm)
  assert.equal(wire.expires_at, '2099-01-01T00:00')
  const policyWire = policyInput(policy, providers)
  assert.deepEqual(policyWire.ip_whitelist, stored.ip_whitelist)
  assert.deepEqual(policyWire.model_quotas, stored.model_quotas)
  assert.deepEqual(policyWire.allowed_groups, stored.allowed_groups)
  assert.deepEqual(policyWire.model_groups, stored.model_groups)
  assert.deepEqual(policyWire.excluded_providers, stored.excluded_providers)
})

test('zero budgets and missing policy fields prefill as unlimited/empty', () => {
  const draft = keyEditDraft({ id: 1, name: 'minimal', models: [], token_limit: 0, max_amount_micros: 0, rpm: 0, tpm: 0, expires_at: null, enabled: true }, [], new Map())
  assert.equal(draft.maxAmount, '')
  assert.equal(draft.tokenLimit, '')
  assert.equal(draft.expiresAt, '')
  assert.equal(draft.modelQuotas, '')
  assert.equal(draft.ipWhitelist, '')
  assert.deepEqual(draft.routeModels, [])
  assert.deepEqual(draft.providerModels, [])
  assert.deepEqual(draft.modelGroups, [])
  assert.deepEqual(draft.allowedGroups, [])
  assert.deepEqual(draft.excludedProviders, [])
  assert.equal(draft.enabled, true)
  const policy = parseKeyPolicy(draft)
  const wire = keyInput(policy)
  assert.equal(wire.token_limit, 0)
  assert.equal(wire.max_amount_micros, 0)
  assert.equal(wire.expires_at, null)
})