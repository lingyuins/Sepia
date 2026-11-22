import assert from 'node:assert/strict'
import test from 'node:test'
import { pluginRpcPath } from './plugin-rpc.ts'

test('plugin RPC methods stay inside their plugin namespace', () => {
  assert.equal(pluginRpcPath('echo', 'status'), '/api/v1/plugins/echo/rpc/status')
  assert.equal(pluginRpcPath('echo', '/nested/action'), '/api/v1/plugins/echo/rpc/nested/action')
  assert.equal(pluginRpcPath('echo', '读取/状态'), '/api/v1/plugins/echo/rpc/%E8%AF%BB%E5%8F%96/%E7%8A%B6%E6%80%81')
})

test('plugin RPC rejects traversal, encoded separators and control characters', () => {
  for (const method of ['../../keys', '..', 'nested/../keys', 'nested\\keys', 'a b?c', 'action?x=1', 'action#fragment', 'action%2Fkeys', 'action%5Ckeys', 'action\nkeys', '', '   ', 'a//b']) {
    assert.throws(() => pluginRpcPath('echo', method), /Invalid plugin RPC method/)
  }
})
