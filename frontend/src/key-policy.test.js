import assert from 'node:assert/strict'
import test from 'node:test'

import { parseKeyPolicy, parseTokenLimit } from './key-policy.ts'

const defaults = { name: 'bot', customValue: '', maxAmount: '', tokenLimit: '', rpm: '0', tpm: '0', modelQuotas: '', ipWhitelist: '', expiresAt: '', routeModels: ['smart-route'], providerModels: ['gpt-4o'], allowedGroups: [], excludedProviders: [], enabled: true }

test('accepts human-friendly token limits', () => {
  assert.equal(parseTokenLimit('100w'), 1000000)
  assert.equal(parseTokenLimit('5亿'), 500000000)
  assert.equal(parseTokenLimit('1000000'), 1000000)
  assert.equal(parseTokenLimit(''), null)
})

test('rejects invalid token limits', () => {
  assert.throws(() => parseTokenLimit('many'), /Token/)
})

test('defaults to no per-model quotas', () => {
  const policy = parseKeyPolicy(defaults)
  assert.deepEqual(policy.modelQuotas, {})
})

test('accepts explicit per-model quotas', () => {
  const policy = parseKeyPolicy({ ...defaults, modelQuotas: '{"gpt-4o":{"rpm":5,"tpm":50000}}' })
  assert.deepEqual(policy.modelQuotas, { 'gpt-4o': { rpm: 5, tpm: 50000 } })
})

test('builds a complete key policy with unlimited optional limits', () => {
  const policy = parseKeyPolicy({ ...defaults, modelQuotas: '{"gpt-4o":{"rpm":5,"tpm":50000}}', maxAmount: '', tokenLimit: '', expiresAt: '' })
  assert.equal(policy.maxAmount, null)
  assert.equal(policy.tokenLimit, null)
  assert.equal(policy.expiresAt, null)
  assert.equal(policy.rpm, 0)
  assert.deepEqual(policy.modelQuotas, { 'gpt-4o': { rpm: 5, tpm: 50000 } })
})

test('rejects invalid per-model quota JSON', () => {
  assert.throws(() => parseKeyPolicy({ ...defaults, modelQuotas: '{"gpt-4o":{"rpm":-1}}' }), /model quota/)
  assert.throws(() => parseKeyPolicy({ ...defaults, modelQuotas: '[]' }), /model quota/)
})

test('validates IPv4, IPv6 and CIDR whitelist entries', () => {
  const policy = parseKeyPolicy({ ...defaults, ipWhitelist: '10.0.0.1, 192.168.0.0/16, ::1, 2001:db8::/32' })
  assert.deepEqual(policy.ipWhitelist, ['10.0.0.1', '192.168.0.0/16', '::1', '2001:db8::/32'])
  assert.throws(() => parseKeyPolicy({ ...defaults, ipWhitelist: '999.0.0.1' }), /IP/)
})

test('normalizes custom keys and expiry dates', () => {
  const policy = parseKeyPolicy({ ...defaults, customValue: '  sk-custom-bot  ', expiresAt: '2027-12-31T00:00' })
  assert.equal(policy.customValue, 'sk-custom-bot')
  assert.equal(policy.expiresAt, '2027-12-31T00:00')
  assert.throws(() => parseKeyPolicy({ ...defaults, customValue: 'contains spaces' }), /custom key/)
})

test('merges the two scope selections into one deduplicated wire model list', () => {
  const policy = parseKeyPolicy({ ...defaults, routeModels: ['qwen4-max', 'smart-route'], providerModels: ['qwen4-max', 'gpt-4o'] })
  assert.deepEqual(policy.models, ['qwen4-max', 'smart-route', 'gpt-4o'])
  assert.equal('routeModels' in policy, false)
  assert.equal('providerModels' in policy, false)
})

test('clearing one scope selection never removes the other', () => {
  const routesOnly = parseKeyPolicy({ ...defaults, routeModels: ['smart-route'], providerModels: [] })
  assert.deepEqual(routesOnly.models, ['smart-route'])
  const providersOnly = parseKeyPolicy({ ...defaults, routeModels: [], providerModels: ['gpt-4o'] })
  assert.deepEqual(providersOnly.models, ['gpt-4o'])
  const bothCleared = parseKeyPolicy({ ...defaults, routeModels: [], providerModels: [] })
  assert.deepEqual(bothCleared.models, [])
})
