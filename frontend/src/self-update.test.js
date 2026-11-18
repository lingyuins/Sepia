import assert from 'node:assert/strict'
import test from 'node:test'

import {
  formatVersion, restartFinished, updateActionEnabled, updateButtonLabel, waitForRestart,
} from './self-update.ts'

const labels = { check: 'Check for updates', checking: 'Checking…', apply: 'Update now', applying: 'Downloading…', restarting: 'Restarting…' }
const check = (overrides = {}) => ({
  current_version: '0.1.0', latest_version: '0.2.0', update_available: true,
  release_notes: 'notes', published_at: null, release_url: 'https://example.test', platform_supported: true,
  ...overrides,
})

test('formatVersion prefixes a real version and dashes an empty one', () => {
  assert.equal(formatVersion('0.1.0'), 'v0.1.0')
  assert.equal(formatVersion(''), '—')
})

test('updateButtonLabel follows the phase, then the check result', () => {
  assert.equal(updateButtonLabel('checking', check(), labels), 'Checking…')
  assert.equal(updateButtonLabel('applying', check(), labels), 'Downloading…')
  assert.equal(updateButtonLabel('restarting', check(), labels), 'Restarting…')
  assert.equal(updateButtonLabel('idle', check(), labels), 'Update now')
  assert.equal(updateButtonLabel('idle', check({ update_available: false }), labels), 'Check for updates')
  assert.equal(updateButtonLabel('idle', null, labels), 'Check for updates')
})

test('updateActionEnabled allows idle checks, re-checks when up to date, and supported updates only', () => {
  assert.equal(updateActionEnabled('idle', null), true)
  assert.equal(updateActionEnabled('idle', check()), true)
  assert.equal(updateActionEnabled('idle', check({ update_available: false })), true)
  assert.equal(updateActionEnabled('idle', check({ platform_supported: false })), false)
  assert.equal(updateActionEnabled('checking', null), false)
  assert.equal(updateActionEnabled('applying', check()), false)
  assert.equal(updateActionEnabled('restarting', check()), false)
})

test('restartFinished needs two real, different versions', () => {
  assert.equal(restartFinished('0.1.0', '0.2.0'), true)
  assert.equal(restartFinished('0.1.0', '0.1.0'), false)
  assert.equal(restartFinished('', '0.2.0'), false)
  assert.equal(restartFinished('0.1.0', ''), false)
})

test('waitForRestart survives fetch failures until the version changes', async () => {
  const versions = ['', '0.1.0', '0.1.0', '0.2.0']
  let calls = 0
  const result = await waitForRestart('0.1.0', async () => {
    const value = versions[calls]
    calls += 1
    if (value === '') throw new Error('gateway is restarting')
    return value
  }, { sleep: async () => {}, now: () => 0, timeoutMs: 1000 })
  assert.equal(result, true)
  assert.equal(calls, 4)
})

test('waitForRestart gives up at the deadline while the gateway stays old', async () => {
  let clock = 0
  const result = await waitForRestart('0.1.0', async () => '0.1.0', {
    sleep: async (ms) => { clock += ms },
    now: () => clock,
    intervalMs: 100,
    timeoutMs: 300,
  })
  assert.equal(result, false)
})

test('waitForRestart tolerates a gateway that never answers', async () => {
  let clock = 0
  const result = await waitForRestart('0.1.0', async () => { throw new Error('down') }, {
    sleep: async (ms) => { clock += ms },
    now: () => clock,
    intervalMs: 100,
    timeoutMs: 200,
  })
  assert.equal(result, false)
})