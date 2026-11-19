import test from 'node:test'
import assert from 'node:assert/strict'
import {
  BUILTIN_FUNCTIONAL_SUFFIXES,
  BUILTIN_ROUTER_PREFIXES,
  compileNormalizeRules,
  dotDashKey,
  normalizeModelName,
  validateNormalizeRules,
} from './model-normalize.ts'
import { groupFamilies } from './model-plaza.ts'

// --- builtin normalization (ported from octopus normalize.ts behavior) ---

test('strips router prefixes and path segments', () => {
  assert.equal(normalizeModelName('dmxapi-kimi-k2.5'), 'kimi-k2.5')
  assert.equal(normalizeModelName('@cf/moonshotai/kimi-k2.5'), 'kimi-k2.5')
  assert.equal(normalizeModelName('openai/gpt-4o'), 'gpt-4o')
  assert.equal(normalizeModelName('anthropic-claude-sonnet-4'), 'claude-sonnet-4')
})

test('strips functional suffixes case-insensitively and repeatedly', () => {
  assert.equal(normalizeModelName('Kimi-K2.5-CC'), 'kimi-k2.5')
  assert.equal(normalizeModelName('dmxapi-kimi-k2.5-cc'), 'kimi-k2.5')
  assert.equal(normalizeModelName('kimi-k2.5:free'), 'kimi-k2.5')
  assert.equal(normalizeModelName('kimi-k2.5(free)'), 'kimi-k2.5')
  assert.equal(normalizeModelName('gpt-4o-free-cc'), 'gpt-4o')
  assert.equal(normalizeModelName('model-latest-vip'), 'model')
})

test('keeps distinct model semantics intact', () => {
  assert.equal(normalizeModelName('gpt-4o-mini'), 'gpt-4o-mini')
  assert.equal(normalizeModelName('gemini-2.5-flash'), 'gemini-2.5-flash')
  assert.equal(normalizeModelName('deepseek-r1-32k'), 'deepseek-r1-32k')
  assert.equal(normalizeModelName('claude-opus-4.6'), 'claude-opus-4.6')
})

test('malformed or empty names never disappear', () => {
  assert.equal(normalizeModelName(''), '')
  assert.equal(normalizeModelName('dmxapi-'), 'dmxapi-')
  assert.equal(normalizeModelName('   '), '')
})

// --- rule customization ---

test('custom prefixes replace builtins; trailing-dash prefix matches bracket form', () => {
  const rules = compileNormalizeRules({ routerPrefixes: ['[官B]-'] })
  assert.equal(normalizeModelName('[官B]-claude-opus-4-6', rules), 'claude-opus-4-6')
  assert.equal(normalizeModelName('[官B]claude-opus-4-6', rules), 'claude-opus-4-6')
  // builtin prefixes are disabled once a custom list is provided
  assert.equal(normalizeModelName('dmxapi-kimi-k2.5', rules), 'dmxapi-kimi-k2.5')
})

test('custom regex suffixes strip dates, -@ variant and alternations', () => {
  const rules = compileNormalizeRules({ functionalSuffixes: ['-\\d{8}', '-@\\d{4}-\\d{2}-\\d{2}', '-beta|-rc'] })
  assert.equal(normalizeModelName('gpt-4o-20240806', rules), 'gpt-4o')
  assert.equal(normalizeModelName('claude-3-5-sonnet-@2024-10-22', rules), 'claude-3-5-sonnet')
  // '-@' prefix also matches the '@' form without the hyphen
  assert.equal(normalizeModelName('claude-3-5-sonnet@2024-10-22', rules), 'claude-3-5-sonnet')
  assert.equal(normalizeModelName('model-beta', rules), 'model')
  assert.equal(normalizeModelName('model-rc', rules), 'model')
})

test('nullable regex suffix cannot loop forever or eat the whole name', () => {
  const rules = compileNormalizeRules({ functionalSuffixes: ['-x*'] })
  assert.equal(normalizeModelName('model-x', rules), 'model')
  assert.equal(normalizeModelName('model-', rules), 'model')
  assert.equal(normalizeModelName('-', rules), '-')
})

test('invalid regex suffix is ignored, literal forms still apply', () => {
  // '-(' is literal (no regex metacharacters), so it strips like '-cc'.
  const literal = compileNormalizeRules({ functionalSuffixes: ['-('] })
  assert.equal(normalizeModelName('model-(', literal), 'model')
  // '-(x*' has '*' → regex candidate, but '(?:-(x*)$' is invalid (unclosed
  // group). The literal form does not match this input, so nothing strips.
  const invalid = compileNormalizeRules({ functionalSuffixes: ['-(x*'] })
  assert.equal(normalizeModelName('model-(x', invalid), 'model-(x')
})

test('explicit mappings win over stripping and match dot-dash equivalently', () => {
  const rules = compileNormalizeRules({ explicitMappings: [{ variant: 'claude-opus-4-6', canonical: 'claude-opus-4.6' }] })
  assert.equal(normalizeModelName('claude-opus-4.6', rules), 'claude-opus-4.6')
  assert.equal(normalizeModelName('Claude-Opus-4-6', rules), 'claude-opus-4.6')
  // base-key match: functional suffix stripped from the variant before lookup
  assert.equal(normalizeModelName('claude-opus-4-6-cc', rules), 'claude-opus-4.6')
  assert.equal(dotDashKey('Claude-Opus-4-6'), 'claude.opus.4.6')
})

test('empty rule lists fall back to builtins', () => {
  const rules = compileNormalizeRules({ routerPrefixes: [], functionalSuffixes: [], explicitMappings: [] })
  assert.deepEqual(rules.routerPrefixes, BUILTIN_ROUTER_PREFIXES)
  assert.deepEqual(rules.functionalSuffixes, BUILTIN_FUNCTIONAL_SUFFIXES)
  assert.equal(normalizeModelName('dmxapi-kimi-k2.5-cc', rules), 'kimi-k2.5')
})

// --- validation (mirrors octopus modelnormalize.ValidateRules) ---

test('validation accepts builtin-shaped rules', () => {
  assert.equal(validateNormalizeRules({ routerPrefixes: ['dmxapi-', '[官B]-'], functionalSuffixes: ['-cc', '-\\d{8}'], explicitMappings: [{ variant: 'Kimi-K2.5-CC', canonical: 'kimi-k2.5' }] }), null)
  assert.equal(validateNormalizeRules({}), null)
})

test('validation rejects unsafe prefixes', () => {
  assert.equal(validateNormalizeRules({ routerPrefixes: [''] }).key, 'normalizeErrPrefixEmpty')
  assert.equal(validateNormalizeRules({ routerPrefixes: ['a/b-'] }).key, 'normalizeErrPrefixPath')
  assert.equal(validateNormalizeRules({ routerPrefixes: ['gpt'] }).key, 'normalizeErrPrefixBoundary')
  assert.equal(validateNormalizeRules({ routerPrefixes: ['dmxapi-', 'DMXAPI-'] }).key, 'normalizeErrPrefixDuplicate')
})

test('validation rejects suffixes that would merge distinct models', () => {
  assert.equal(validateNormalizeRules({ functionalSuffixes: [''] }).key, 'normalizeErrSuffixEmpty')
  assert.equal(validateNormalizeRules({ functionalSuffixes: ['-mini'] }).key, 'normalizeErrSuffixProtected')
  assert.equal(validateNormalizeRules({ functionalSuffixes: ['-pro'] }).key, 'normalizeErrSuffixProtected')
  assert.equal(validateNormalizeRules({ functionalSuffixes: ['-\\d+k'] }).key, 'normalizeErrSuffixContext')
  assert.equal(validateNormalizeRules({ functionalSuffixes: ['-(x*'] }).key, 'normalizeErrSuffixRegex')
  // literal '(free)' is not treated as a regex group
  assert.equal(validateNormalizeRules({ functionalSuffixes: ['-(free)'] }), null)
})

test('validation rejects malformed or conflicting mappings', () => {
  assert.equal(validateNormalizeRules({ explicitMappings: [{ variant: '', canonical: 'x' }] }).key, 'normalizeErrMappingEmpty')
  assert.equal(validateNormalizeRules({ explicitMappings: [{ variant: 'a', canonical: 'Bad Name' }] }).key, 'normalizeErrMappingCanonical')
  assert.equal(validateNormalizeRules({ explicitMappings: [{ variant: 'a', canonical: 'model-cc' }] }).key, 'normalizeErrMappingUnstable')
  const conflict = validateNormalizeRules({ explicitMappings: [{ variant: 'claude-opus-4-6', canonical: 'claude-opus-4.6' }, { variant: 'claude-opus-4.6', canonical: 'claude-opus-4-6' }] })
  assert.equal(conflict.key, 'normalizeErrMappingConflict')
  // same canonical on dot and dash forms is not a conflict
  assert.equal(validateNormalizeRules({ explicitMappings: [{ variant: 'claude-opus-4-6', canonical: 'claude-opus-4.6' }, { variant: 'claude-opus-4.6', canonical: 'claude-opus-4.6' }] }), null)
})

// --- plaza integration ---

const channel = { providerId: 1, provider: 'Demo', groupId: 'official', protocol: 'openai', enabled: true, keyCount: 1, inputPriceMicros: null, outputPriceMicros: null, routes: [] }
const variant = (model, baseModel) => ({ model, baseModel, metadata: null, channels: [channel] })

test('groupFamilies merges router-prefixed and suffixed variants into one family', () => {
  const families = groupFamilies([
    variant('kimi-k2.5', 'kimi-k2.5'),
    variant('dmxapi-kimi-k2.5', 'dmxapi-kimi-k2.5'),
    variant('kimi-k2.5-cc', 'kimi-k2.5'),
    variant('gpt-4o-mini', 'gpt-4o-mini'),
  ])
  assert.equal(families.length, 2)
  const kimi = families.find((family) => family.base === 'kimi-k2.5')
  assert.equal(kimi.variants.length, 3)
  assert.equal(kimi.channels.length, 3)
})

test('groupFamilies applies explicit mappings from settings rules', () => {
  const families = groupFamilies([
    variant('claude-opus-4-6', 'claude-opus-4-6'),
    variant('claude-opus-4.6', 'claude-opus-4.6'),
  ], { explicitMappings: [{ variant: 'claude-opus-4-6', canonical: 'claude-opus-4.6' }] })
  assert.equal(families.length, 1)
  assert.equal(families[0].base, 'claude-opus-4.6')
})

test('groupFamilies default rules keep backend base ids unchanged for ordinary models', () => {
  const families = groupFamilies([variant('gpt-x-2024-08-06', 'gpt-x'), variant('gpt-x', 'gpt-x')])
  assert.equal(families.length, 1)
  assert.equal(families[0].base, 'gpt-x')
})