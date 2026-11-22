import assert from 'node:assert/strict'
import test from 'node:test'
import { addSelectedModels, filterModels, mergeDetectedModels } from './fetch-models.ts'

test('merges per-key catalogs into a de-duplicated union, first-seen order', () => {
  const results = [
    { keyId: 1, models: ['gpt-a', 'gpt-b'], error: null },
    { keyId: 2, models: ['gpt-b', 'gpt-c'], error: null },
    { keyId: 3, models: null, error: 'upstream HTTP 401' },
  ]
  assert.deepEqual(mergeDetectedModels(results), ['gpt-a', 'gpt-b', 'gpt-c'])
  assert.deepEqual(mergeDetectedModels([{ keyId: 1, models: null, error: 'offline' }]), [])
  assert.deepEqual(mergeDetectedModels([]), [])
})

test('one search query filters both the union and per-key lists case-insensitively', () => {
  const models = ['gpt-4o', 'claude-sonnet', 'GPT-4o-mini', 'gemini-flash']
  assert.deepEqual(filterModels(models, 'gpt'), ['gpt-4o', 'GPT-4o-mini'])
  assert.deepEqual(filterModels(models, '  CLAUDE  '), ['claude-sonnet'])
  assert.deepEqual(filterModels(models, ''), models)
  assert.deepEqual(filterModels(models, '   '), models)
  assert.deepEqual(filterModels(models, 'no-match'), [])
})

test('adding a selection keeps configured order and never duplicates', () => {
  assert.deepEqual(addSelectedModels(['gpt-a'], ['gpt-a', 'gpt-b', 'gpt-b']), ['gpt-a', 'gpt-b'])
  assert.deepEqual(addSelectedModels([], ['x', 'y']), ['x', 'y'])
  assert.deepEqual(addSelectedModels(['x'], []), ['x'])
})