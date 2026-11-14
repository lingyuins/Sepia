import assert from 'node:assert/strict'
import test from 'node:test'

import { clearLegacyOnboarding, ONBOARDING_KEY } from './onboarding.ts'

test('clears only the legacy onboarding marker', () => {
  const values = new Map([[ONBOARDING_KEY, 'complete'], ['sepia.workspace.v1', '{"settings":{"language":"日本語"}}']])
  globalThis.localStorage = {
    removeItem(key) {
      values.delete(key)
    },
  }
  clearLegacyOnboarding()
  assert.equal(values.has(ONBOARDING_KEY), false)
  assert.equal(values.get('sepia.workspace.v1'), '{"settings":{"language":"日本語"}}')
})

test('storage failures do not block gateway startup', () => {
  globalThis.localStorage = {
    removeItem() {
      throw new Error('storage disabled')
    },
  }
  assert.doesNotThrow(clearLegacyOnboarding)
})
