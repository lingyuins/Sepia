import assert from 'node:assert/strict'
import test from 'node:test'

import { selectOverviewRoute, latestSuccessfulRouteRequest, routeDisplayCandidates, duplicateRouteIdentity } from './route-display.ts'

const routes = [
  { id: 1, name: 'Unused', slug: 'unused', active: true, primary: 'a', primaryModel: 'same-model', fallback: '', fallbackModel: '', candidates: [{ id: '1-a', provider: 'a', model: 'same-model', enabled: true, weight: 1 }] },
  { id: 2, name: 'Recent', slug: 'recent', active: true, primary: 'b', primaryModel: 'same-model', fallback: 'c', fallbackModel: 'same-model', candidates: [{ id: '2-b', provider: 'b', model: 'same-model', enabled: true, weight: 1 }, { id: '2-c', provider: 'c', model: 'same-model', enabled: true, weight: 1 }] },
  { id: 3, name: 'Paused', slug: 'paused', active: false, primary: 'd', primaryModel: 'other', fallback: '', fallbackModel: '', candidates: [{ id: '3-d', provider: 'd', model: 'other', enabled: true, weight: 1 }] },
]
const requests = [
  { id: 'r1', createdAt: '2026-11-13T10:00:00Z', route: 'paused', model: 'other', status: '200 OK' },
  { id: 'r2', createdAt: '2026-11-13T12:00:00Z', route: 'recent', model: 'same-model', status: '200 OK' },
  { id: 'r3', createdAt: '2026-11-13T11:00:00Z', route: 'unused', model: 'same-model', status: 'denied' },
]

test('prefers the enabled route with the most recent request for the overview', () => {
  assert.equal(selectOverviewRoute(routes, requests)?.slug, 'recent')
})

test('uses request timestamps when older route requests appear first', () => {
  assert.equal(selectOverviewRoute(routes, [requests[2], requests[1]])?.slug, 'recent')
})

test('ignores newer unrouted and disabled-route requests when choosing the overview', () => {
  assert.equal(selectOverviewRoute(routes, [requests[1], { ...requests[0], id: 'legacy', createdAt: '2026-11-13T13:00:00Z', route: '—' }])?.slug, 'recent')
  assert.equal(selectOverviewRoute(routes, [requests[1], { ...requests[0], id: 'disabled', createdAt: '2026-11-13T14:00:00Z', route: 'paused' }])?.slug, 'recent')
})

test('falls back to the first enabled route when no requests exist', () => {
  assert.equal(selectOverviewRoute(routes, [])?.slug, 'unused')
  assert.equal(selectOverviewRoute([routes[2]], []) , undefined)
})

test('only uses a successful request for the selected route', () => {
  assert.equal(latestSuccessfulRouteRequest(routes[0], requests), undefined)
  assert.equal(latestSuccessfulRouteRequest(routes[1], requests)?.model, 'same-model')
})

test('keeps same-model providers as separate ordered candidates', () => {
  assert.deepEqual(routeDisplayCandidates(routes[1]).map(({ provider, model }) => ({ provider, model })), [
    { provider: 'b', model: 'same-model' }, { provider: 'c', model: 'same-model' },
  ])
})

test('duplicating a route appends -copy and the localized copy suffix', () => {
  assert.deepEqual(duplicateRouteIdentity(routes[1], ['recent'], '（副本）'), { slug: 'recent-copy', name: 'Recent （副本）' })
})

test('repeated copies gain numeric suffixes against every existing slug', () => {
  assert.deepEqual(duplicateRouteIdentity(routes[1], ['recent', 'recent-copy'], '（副本）'), { slug: 'recent-copy-2', name: 'Recent （副本） 2' })
  assert.deepEqual(duplicateRouteIdentity(routes[1], ['recent', 'recent-copy', 'recent-copy-2'], '（副本）'), { slug: 'recent-copy-3', name: 'Recent （副本） 3' })
  // A collision on -copy-2 alone still skips it.
  assert.deepEqual(duplicateRouteIdentity(routes[1], ['recent', 'recent-copy-2'], '(copy)'), { slug: 'recent-copy', name: 'Recent (copy)' })
})

test('duplicate slugs stay within 64 characters by shortening the base', () => {
  const long = { ...routes[1], slug: 'x'.repeat(60) }
  const first = duplicateRouteIdentity(long, [long.slug], '(copy)')
  assert.equal(first.slug.length, 64)
  assert.equal(first.slug.endsWith('-copy'), true)
  const second = duplicateRouteIdentity(long, [long.slug, first.slug], '(copy)')
  assert.equal(second.slug.length, 64)
  assert.equal(second.slug.endsWith('-copy-2'), true)
})

test('a route without a slug falls back to a slugified name', () => {
  const named = { ...routes[1], slug: undefined, name: 'Qwen4 Max' }
  assert.deepEqual(duplicateRouteIdentity(named, [], '(copy)'), { slug: 'qwen4-max-copy', name: 'Qwen4 Max (copy)' })
})
