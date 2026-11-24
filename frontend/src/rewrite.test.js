import assert from 'node:assert/strict'
import test from 'node:test'

import {
  formatParamValue,
  isReservedHeader,
  mergeRewrite,
  parseParamValue,
  rewriteCount,
  rewriteOf,
  testRegex,
  validateRewrite,
} from './rewrite.ts'

test('rewriteOf normalizes missing and partial sources', () => {
  assert.deepEqual(rewriteOf(undefined), { headers: [], paramReplace: [], promptReplace: [], paramDefaults: [] })
  assert.deepEqual(rewriteOf(null), { headers: [], paramReplace: [], promptReplace: [], paramDefaults: [] })
  assert.deepEqual(rewriteOf({ promptReplace: [{ pattern: 'a', replacement: 'b' }] }), {
    headers: [],
    paramReplace: [],
    promptReplace: [{ pattern: 'a', replacement: 'b' }],
    paramDefaults: [],
  })
  const source = { headers: [{ name: 'X-A', value: '1' }], paramReplace: [], promptReplace: [], paramDefaults: [] }
  const copy = rewriteOf(source)
  copy.headers[0].value = '2'
  assert.equal(source.headers[0].value, '1')
})

test('rewriteCount counts every rule across the four groups', () => {
  assert.equal(rewriteCount(undefined), 0)
  assert.equal(
    rewriteCount({
      headers: [{ name: 'x-a', value: '1' }, { name: 'x-b', value: '2' }],
      paramReplace: [{ field: 'model', pattern: 'a', replacement: 'b' }],
      promptReplace: [{ pattern: 'c', replacement: 'd' }],
      paramDefaults: [{ field: 'temperature', value: 0.2 }],
    }),
    5,
  )
})

test('mergeRewrite lets the route layer win per key without concatenating', () => {
  const provider = {
    headers: [
      { name: 'User-Agent', value: 'provider-agent' },
      { name: 'X-Provider', value: '1' },
    ],
    paramReplace: [
      { field: 'model', pattern: 'provider', replacement: 'p' },
      { field: 'top_p', pattern: '1', replacement: '1' },
    ],
    promptReplace: [{ pattern: 'provider', replacement: 'p' }],
    paramDefaults: [
      { field: 'temperature', value: 0.2 },
      { field: 'top_k', value: 10 },
    ],
  }
  const route = {
    headers: [{ name: 'user-agent', value: 'route-agent' }],
    paramReplace: [{ field: ' model ', pattern: 'route', replacement: 'r' }],
    promptReplace: [
      { pattern: 'provider', replacement: 'r' },
      { pattern: 'route-only', replacement: 'x' },
    ],
    paramDefaults: [{ field: 'temperature', value: 0.9 }],
  }
  const merged = mergeRewrite(provider, route)
  assert.deepEqual(merged.headers, [
    { name: 'user-agent', value: 'route-agent' },
    { name: 'X-Provider', value: '1' },
  ])
  assert.deepEqual(merged.paramReplace, [
    { field: ' model ', pattern: 'route', replacement: 'r' },
    { field: 'top_p', pattern: '1', replacement: '1' },
  ])
  assert.deepEqual(merged.promptReplace, [
    { pattern: 'provider', replacement: 'r' },
    { pattern: 'route-only', replacement: 'x' },
  ])
  assert.deepEqual(merged.paramDefaults, [
    { field: 'temperature', value: 0.9 },
    { field: 'top_k', value: 10 },
  ])
})

test('mergeRewrite with an empty route keeps provider rules and vice versa', () => {
  const provider = { paramDefaults: [{ field: 'max_tokens', value: 4096 }] }
  assert.deepEqual(mergeRewrite(provider, undefined).paramDefaults, [{ field: 'max_tokens', value: 4096 }])
  assert.deepEqual(mergeRewrite(undefined, provider).paramDefaults, [{ field: 'max_tokens', value: 4096 }])
  assert.deepEqual(mergeRewrite(undefined, undefined), { headers: [], paramReplace: [], promptReplace: [], paramDefaults: [] })
})

test('isReservedHeader ignores case and surrounding blanks', () => {
  assert.equal(isReservedHeader('Authorization'), true)
  assert.equal(isReservedHeader('  X-API-KEY '), true)
  assert.equal(isReservedHeader('Cookie'), true)
  assert.equal(isReservedHeader('Anthropic-Version'), true)
  assert.equal(isReservedHeader('user-agent'), false)
  assert.equal(isReservedHeader('x-custom-header'), false)
  assert.equal(isReservedHeader(''), false)
})

test('testRegex applies global replacement and capture groups', () => {
  assert.equal(testRegex('qwen', 'gpt', 'qwen-max qwen-plus'), 'gpt-max gpt-plus')
  assert.equal(testRegex('^(.*)-preview$', '$1', 'gpt-4o-preview'), 'gpt-4o')
  assert.equal(testRegex('(?<model>\\w+)$', '${model}-v2', 'gpt-4o'), 'gpt-4o-v2')
  assert.throws(() => testRegex('(', '', 'x'), SyntaxError)
})

test('param default values round-trip through JSON text', () => {
  assert.equal(parseParamValue(''), undefined)
  assert.equal(parseParamValue('   '), undefined)
  assert.equal(parseParamValue('16'), 16)
  assert.equal(parseParamValue('true'), true)
  assert.deepEqual(parseParamValue('{"a":1}'), { a: 1 })
  assert.equal(parseParamValue('not json'), undefined)
  assert.equal(formatParamValue(undefined), '')
  assert.equal(formatParamValue(16), '16')
  assert.equal(formatParamValue({ a: 1 }), '{"a":1}')
  assert.deepEqual(parseParamValue(formatParamValue({ a: [1, 2] })), { a: [1, 2] })
})

test('validateRewrite accepts an empty or fully valid config', () => {
  assert.deepEqual(validateRewrite(undefined), [])
  assert.deepEqual(
    validateRewrite({
      headers: [{ name: 'User-Agent', value: 'x' }],
      paramReplace: [{ field: 'model', pattern: '^qwen', replacement: 'gpt' }],
      promptReplace: [{ pattern: 'foo', replacement: 'bar' }],
      paramDefaults: [
        { field: 'temperature', value: 0.2 },
        { field: 'max_tokens', value: 4096 },
      ],
    }),
    [],
  )
})

test('validateRewrite rejects reserved headers and blank names', () => {
  const issues = validateRewrite({
    headers: [
      { name: 'Cookie', value: 'a=1' },
      { name: '   ', value: 'x' },
    ],
  })
  assert.deepEqual(issues, [
    { key: 'rewriteHeaderReserved', values: { name: 'Cookie' } },
    { key: 'rewriteHeaderNameRequired' },
  ])
})

test('validateRewrite rejects padded header names and line-broken values', () => {
  const issues = validateRewrite({
    headers: [
      { name: ' X-Spaced ', value: 'ok' },
      { name: 'X-Broken', value: 'a\r\nb' },
      { name: 'X-Lf', value: 'a\nb' },
    ],
  })
  assert.deepEqual(issues, [
    { key: 'rewriteHeaderNameSpaces', values: { name: ' X-Spaced ' } },
    { key: 'rewriteHeaderValueLine', values: { name: 'X-Broken' } },
    { key: 'rewriteHeaderValueLine', values: { name: 'X-Lf' } },
  ])
})

test('validateRewrite reports empty and invalid patterns', () => {
  const issues = validateRewrite({
    paramReplace: [
      { field: 'model', pattern: '', replacement: 'x' },
      { field: 'model', pattern: '([a-z', replacement: 'x' },
    ],
    promptReplace: [{ pattern: ' ', replacement: 'x' }],
  })
  assert.deepEqual(issues, [
    { key: 'rewritePatternRequired' },
    { key: 'rewriteInvalidPattern', values: { pattern: '([a-z' } },
    { key: 'rewritePatternRequired' },
  ])
})

test('validateRewrite requires default fields and JSON values', () => {
  const issues = validateRewrite({
    paramDefaults: [
      { field: ' ', value: 1 },
      { field: 'temperature', value: undefined },
    ],
  })
  assert.deepEqual(issues, [
    { key: 'rewriteFieldRequired' },
    { key: 'rewriteDefaultInvalid', values: { field: 'temperature' } },
  ])
})
