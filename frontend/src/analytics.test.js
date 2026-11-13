import assert from 'node:assert/strict'
import test from 'node:test'

import { chartTimeLabel } from './analytics.ts'

test('formats hourly analytics buckets as local clock time', () => {
  assert.equal(chartTimeLabel('2026-11-13T01:00:00Z', '24 hours', 'en-US', 'UTC'), '01:00')
})

test('formats daily analytics buckets as local month and day', () => {
  assert.equal(chartTimeLabel('2026-11-13T00:00:00Z', '30 days', 'zh-CN', 'UTC'), '11/13')
})

test('keeps an invalid timestamp readable instead of throwing', () => {
  assert.equal(chartTimeLabel('not-a-date', '7 days', 'en-US', 'UTC'), 'not-a-date')
})
