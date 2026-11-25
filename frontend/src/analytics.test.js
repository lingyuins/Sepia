import assert from 'node:assert/strict'
import test from 'node:test'

import { analyticsPeriodLabelKey, chartTimeLabel, modelPerformanceRows, rankRoutePerformance, routeAggregate } from './analytics.ts'

test('formats hourly analytics buckets as local clock time', () => {
  assert.equal(chartTimeLabel('2026-11-13T01:00:00Z', '24 hours', 'en-US', 'UTC'), '01:00')
})

test('formats daily analytics buckets as local month and day', () => {
  assert.equal(chartTimeLabel('2026-11-13T00:00:00Z', '30 days', 'zh-CN', 'UTC'), '11/13')
})

test('keeps an invalid timestamp readable instead of throwing', () => {
  assert.equal(chartTimeLabel('not-a-date', '7 days', 'en-US', 'UTC'), 'not-a-date')
})

test('route health comes from the server-side window aggregate', () => {
  const rows = [
    { route: 'busy', requests: 12, successful: 9, tokens: 480, latency_ms: 120.4 },
    { route: 'steady', requests: 3, successful: 3, tokens: 45, latency_ms: 24.6 },
  ]
  assert.deepEqual(routeAggregate(rows, 'steady'), { requests: 3, latency: 25, tokens: 45, success: '100.00%' })
  assert.deepEqual(routeAggregate(rows, 'busy'), { requests: 12, latency: 120, tokens: 480, success: '75.00%' })
})

test('route health is empty when the aggregate has no row for the route', () => {
  assert.deepEqual(routeAggregate(undefined, 'steady'), { requests: 0, latency: 0, tokens: 0, success: '—' })
  assert.deepEqual(routeAggregate([{ route: 'empty', requests: 0, successful: 0, tokens: 0, latency_ms: 0 }], 'empty'), {
    requests: 0,
    latency: 0,
    tokens: 0,
    success: '—',
  })
})

test('model performance ranks the server aggregate and keeps the top six', () => {
  const rows = [4, 9, 1, 7, 2, 11, 5].map((requests, index) => ({
    model: index === 5 ? 'claude-sonnet' : `model-${index}`,
    requests,
  }))
  const top = modelPerformanceRows(rows)
  assert.deepEqual(top.map((row) => row.requests), [11, 9, 7, 5, 4, 2])
  assert.equal(top.length, 6)
  assert.equal(top[0].model, 'sonnet')
  assert.deepEqual(modelPerformanceRows(undefined), [])
})

test('labels every analytics period, including 90 days and all time', () => {
  assert.equal(analyticsPeriodLabelKey('90 days'), '90d')
  assert.equal(analyticsPeriodLabelKey('all'), 'allTime')
  assert.equal(analyticsPeriodLabelKey('24 hours'), '24h')
  assert.equal(analyticsPeriodLabelKey('unknown'), '30d')
})

test('route performance drops idle routes and ranks the rest by requests', () => {
  const ranked = rankRoutePerformance([
    { id: 1, requests: 3 },
    { id: 2, requests: 0 },
    { id: 3, requests: 12 },
    { id: 4, requests: 1 },
  ])
  assert.deepEqual(ranked.ranked.map((row) => row.id), [3, 1, 4])
  assert.equal(ranked.hasMore, false)
})

test('route performance charts only the top five routes', () => {
  const rows = [6, 1, 9, 4, 2, 7].map((requests, index) => ({ id: index, requests }))
  const ranked = rankRoutePerformance(rows)
  assert.deepEqual(ranked.top.map((row) => row.requests), [9, 7, 6, 4, 2])
  assert.equal(ranked.ranked.length, 6)
  assert.equal(ranked.hasMore, true)
})

test('route performance offers no toggle with exactly five active routes', () => {
  const ranked = rankRoutePerformance([5, 4, 3, 2, 1, 0].map((requests, index) => ({ id: index, requests })))
  assert.equal(ranked.ranked.length, 5)
  assert.equal(ranked.top.length, 5)
  assert.equal(ranked.hasMore, false)
})

test('route performance is empty when every route is idle', () => {
  const ranked = rankRoutePerformance([{ id: 1, requests: 0 }, { id: 2, requests: 0 }])
  assert.deepEqual(ranked.ranked, [])
  assert.deepEqual(ranked.top, [])
  assert.equal(ranked.hasMore, false)
})
