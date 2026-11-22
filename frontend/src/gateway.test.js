import assert from 'node:assert/strict'
import test from 'node:test'

import { gatewayDisplayHost, gatewayUrl } from './gateway.ts'
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

test('cloud HTTPS uses the standard port regardless of the stored port', () => {
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '443' }),
    'https://gateway.example.com/v1',
  )
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '5000' }),
    'https://gateway.example.com/v1',
  )
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '' }, '/v1/models'),
    'https://gateway.example.com/v1/models',
  )
})

test('cloud HTTP keeps the explicit port in the URL', () => {
  assert.equal(
    gatewayUrl({ deployment: 'cloud', protocol: 'http', host: 'gateway.example.com', port: '8080' }, '/v1/models'),
    'http://gateway.example.com:8080/v1/models',
  )
})

test('cloud HTTPS display hides the stored port', () => {
  assert.equal(
    gatewayDisplayHost({ deployment: 'cloud', protocol: 'https', host: 'sepia.example.com', port: '5000' }),
    'sepia.example.com',
  )
})

test('local and cloud HTTP display keep the explicit port', () => {
  assert.equal(
    gatewayDisplayHost({ deployment: 'local', protocol: 'http', host: '127.0.0.1', port: '5000' }),
    '127.0.0.1:5000',
  )
  assert.equal(
    gatewayDisplayHost({ deployment: 'cloud', protocol: 'http', host: 'gateway.example.com', port: '8080' }),
    'gateway.example.com:8080',
  )
  assert.equal(
    gatewayDisplayHost({ deployment: 'cloud', protocol: 'http', host: 'gateway.example.com', port: '80' }),
    'gateway.example.com',
  )
})
