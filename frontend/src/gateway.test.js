import assert from 'node:assert/strict'
import test from 'node:test'

import { gatewayUrl } from './gateway.ts'
import { defaultSettings } from './settings.ts'

test('defaults the local gateway to port 5000', () => {
  assert.equal(defaultSettings.port, '5000')
})

test('keeps local deployments on HTTP', () => {
  assert.equal(
    gatewayUrl({ deployment: 'local', protocol: 'https', host: '127.0.0.1', port: '3425' }),
    'http://127.0.0.1:3425/v1',
  )
})

test('allows cloud deployments to use HTTP', () => {
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'http', host: 'gateway.example.com', port: '80' }),
    'http://gateway.example.com/v1',
  )
})

test('allows cloud deployments to use HTTPS', () => {
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '443' }),
    'https://gateway.example.com/v1',
  )
})

test('builds a cloud HTTPS URL with a custom port and API path', () => {
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '8443' }, '/v1/models'),
    'https://gateway.example.com:8443/v1/models',
  )
})
