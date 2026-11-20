import { test } from 'node:test'
import assert from 'node:assert/strict'
import { buildModelNamesExport, candidateCoverage, modelChannelsFilename, parseRulesFile } from './normalize-workflow.ts'

test('export deduplicates, trims and sorts model names', () => {
  const result = buildModelNamesExport([' gpt-x ', 'kimi-k2', 'gpt-x', '', '  ', 'Kimi-K2'], new Date('2026-11-19T07:59:06.077Z'))
  assert.deepEqual(result, { exported_at: '2026-11-19T07:59:06.077Z', model_names: ['Kimi-K2', 'gpt-x', 'kimi-k2'] })
})

test('export filename uses local time with zero padding', () => {
  assert.equal(modelChannelsFilename(new Date(2026, 10, 19, 9, 5, 6)), 'model-channels-20261119090506.json')
})

test('rules file must contain exactly the three rule arrays', () => {
  assert.deepEqual(parseRulesFile('not json'), { ok: false, error: 'shape' })
  assert.deepEqual(parseRulesFile('[]'), { ok: false, error: 'shape' })
  assert.deepEqual(parseRulesFile('{"router_prefixes":[]}'), { ok: false, error: 'shape' })
  const extra = { router_prefixes: [], functional_suffixes: [], explicit_mappings: [], extra: 1 }
  assert.deepEqual(parseRulesFile(JSON.stringify(extra)), { ok: false, error: 'shape' })
  const wrongType = { router_prefixes: 'dmxapi-', functional_suffixes: [], explicit_mappings: [] }
  assert.deepEqual(parseRulesFile(JSON.stringify(wrongType)), { ok: false, error: 'arrays' })
})

test('any invalid entry rejects the whole file', () => {
  const base = { router_prefixes: [], functional_suffixes: [], explicit_mappings: [] }
  assert.deepEqual(parseRulesFile(JSON.stringify({ ...base, router_prefixes: ['ok', 3] })), { ok: false, error: 'entries' })
  assert.deepEqual(parseRulesFile(JSON.stringify({ ...base, functional_suffixes: [' '] })), { ok: false, error: 'entries' })
  assert.deepEqual(parseRulesFile(JSON.stringify({ ...base, explicit_mappings: [{ variant: 'a', canonical: '' }] })), { ok: false, error: 'entries' })
  assert.deepEqual(parseRulesFile(JSON.stringify({ ...base, explicit_mappings: ['a'] })), { ok: false, error: 'entries' })
})

test('rules file normalizes separators, lowercases canonicals and deduplicates', () => {
  const file = {
    router_prefixes: ['dmxapi', 'agent-', '[官B]-', '\\[[^\\]]+\\]-?', 'dmxapi'],
    functional_suffixes: ['cc', '-free', 'cc', '\\d{8}'],
    explicit_mappings: [
      { variant: ' dmxapi-clo-4-6 ', canonical: 'Claude-Opus-4.6' },
      { variant: 'dmxapi-clo-4-6', canonical: 'claude-opus-4.6' },
      { variant: 'gpt-y', canonical: 'gpt-x' },
    ],
  }
  const result = parseRulesFile(JSON.stringify(file))
  assert.equal(result.ok, true)
  assert.deepEqual(result.rules, {
    routerPrefixes: ['dmxapi-', 'agent-', '[官B]-', '\\[[^\\]]+\\]-?'],
    functionalSuffixes: ['-cc', '-free', '-\\d{8}'],
    explicitMappings: [
      { variant: 'dmxapi-clo-4-6', canonical: 'claude-opus-4.6' },
      { variant: 'gpt-y', canonical: 'gpt-x' },
    ],
  })
})

test('candidate coverage counts inputs, canonical names and merged variants', () => {
  const coverage = candidateCoverage(['a-cc', 'a', 'b-free', 'c', ' ', ''], (name) => name.replace(/-cc$|-free$/, ''))
  assert.deepEqual(coverage, { input: 4, canonical: 3, merged: 1 })
  assert.deepEqual(candidateCoverage([], () => 'x'), { input: 0, canonical: 0, merged: 0 })
})

test('candidate coverage deduplicates input names before counting merges', () => {
  // Duplicate names across providers are not merges; identity rules merge nothing.
  assert.deepEqual(candidateCoverage(['gpt-x', 'gpt-x', ' gpt-x ', 'gpt-y', 'gpt-y'], (name) => name), { input: 2, canonical: 2, merged: 0 })
  // Real merging still counts once per distinct input name.
  assert.deepEqual(candidateCoverage(['a-cc', 'a', 'a-cc', 'a'], (name) => name.replace(/-cc$/, '')), { input: 2, canonical: 1, merged: 1 })
})