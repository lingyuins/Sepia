import assert from 'node:assert/strict'
import test from 'node:test'
import { translations } from './i18n.ts'
import { translate } from './translation.ts'

test('every translation has a non-empty value in all three languages', () => {
  for (const [key, values] of Object.entries(translations)) {
    assert.equal(values.length, 3, key)
    for (const value of values) {
      assert.equal(typeof value, 'string', key)
      assert.ok(value.trim(), key)
    }
  }
})

test('translation replaces every occurrence of each placeholder', () => {
  const dictionary = { ...translations, repeat: ['{name} → {name}', '{name} → {name}', '{name} → {name}'] }
  assert.equal(translate(dictionary, 'English', 'repeat', { name: 'Alpha' }), 'Alpha → Alpha')
  assert.equal(translate(dictionary, '简体中文', 'repeat', { name: '测试' }), '测试 → 测试')
})
