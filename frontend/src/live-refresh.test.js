import assert from 'node:assert/strict'
import test from 'node:test'
import { LiveRefreshCoalescer, parseLiveEvent } from './live-refresh.ts'

test('parses known workspace domains and drops unknown ones', () => {
  assert.deepEqual(parseLiveEvent('{"domains":["requests","analytics"]}'), ['requests', 'analytics'])
  assert.deepEqual(parseLiveEvent('{"domains":["modelGroups","users","nope"]}'), ['modelGroups'])
  assert.deepEqual(parseLiveEvent('{"domains":["routes","routes"]}'), ['routes'])
})

test('empty, "all" and shapeless frames ask for a full refresh', () => {
  assert.equal(parseLiveEvent('{"domains":[]}'), 'all')
  assert.equal(parseLiveEvent('{"domains":["all"]}'), 'all')
  assert.equal(parseLiveEvent('{"domains":"routes"}'), 'all')
  assert.equal(parseLiveEvent('null'), 'all')
})

test('malformed and wholly unknown frames are ignored', () => {
  assert.equal(parseLiveEvent('not json'), null)
  assert.equal(parseLiveEvent('{"domains":["users","version"]}'), null)
})

test('coalesces a burst into one refresh with merged domains', async () => {
  const calls = []
  const coalescer = new LiveRefreshCoalescer(async (domains, background) => {
    calls.push({ domains, background })
  }, 5)
  const first = coalescer.schedule(['routes'])
  const second = coalescer.schedule(['providers', 'requests'])
  const third = coalescer.schedule(['routes'])
  await Promise.all([first, second, third])
  assert.deepEqual(calls, [{ domains: ['routes', 'providers', 'requests'], background: true }])
})

test('a full-refresh hint subsumes the collected domains', async () => {
  const calls = []
  const coalescer = new LiveRefreshCoalescer(async (domains, background) => {
    calls.push({ domains, background })
  }, 5)
  await Promise.all([coalescer.schedule(['keys']), coalescer.schedule('all')])
  assert.deepEqual(calls, [{ domains: 'all', background: true }])
})

test('any foreground caller turns the batch foreground and sees its errors', async () => {
  const calls = []
  const coalescer = new LiveRefreshCoalescer(async (domains, background) => {
    calls.push({ domains, background })
    throw new Error('gateway offline')
  }, 5)
  const background = coalescer.schedule(['keys'])
  background.catch(() => {})
  const foreground = coalescer.schedule(['keys'], false)
  await assert.rejects(foreground, /gateway offline/)
  await assert.rejects(background, /gateway offline/)
  assert.deepEqual(calls, [{ domains: ['keys'], background: false }])
})

test('cancel drops a pending batch and releases its waiters', async () => {
  let calls = 0
  const coalescer = new LiveRefreshCoalescer(async () => {
    calls += 1
  }, 5)
  const pending = coalescer.schedule(['settings'])
  coalescer.cancel()
  await pending
  await new Promise((resolve) => setTimeout(resolve, 20))
  assert.equal(calls, 0)
})