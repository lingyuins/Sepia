import assert from 'node:assert/strict'
import test from 'node:test'

import {
  authenticationRequest, authenticationResponse, base64urlToBytes, bytesToBase64url,
  passkeySupported, registrationRequest, registrationResponse,
} from './passkey.ts'

test('base64url round-trips bytes including the +/ and padding edge characters', () => {
  const bytes = new Uint8Array([0xfb, 0xff, 0x00, 0x3e, 0x7f, 0x80])
  const encoded = bytesToBase64url(bytes)
  assert.equal(encoded.includes('+'), false)
  assert.equal(encoded.includes('/'), false)
  assert.equal(encoded.includes('='), false)
  assert.deepEqual(base64urlToBytes(encoded), bytes)
  for (let length = 0; length < 12; length += 1) {
    const sample = new Uint8Array(Array.from({ length }, (_, index) => (index * 37 + 251) % 256))
    assert.deepEqual(base64urlToBytes(bytesToBase64url(sample)), sample)
  }
  assert.deepEqual(base64urlToBytes(''), new Uint8Array())
})

test('registrationRequest decodes challenge, user handle and excluded credential ids', () => {
  const challenge = bytesToBase64url(new Uint8Array([1, 2, 3, 250]))
  const handle = bytesToBase64url(new Uint8Array([9, 8, 7]))
  const excluded = bytesToBase64url(new Uint8Array([4, 5, 6]))
  const request = registrationRequest({
    challenge,
    rp: { id: 'localhost', name: 'Sepia' },
    user: { id: handle, name: 'admin', displayName: 'admin' },
    pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
    excludeCredentials: [{ type: 'public-key', id: excluded, transports: ['internal'] }],
    authenticatorSelection: { residentKey: 'required', userVerification: 'required' },
  })
  assert.deepEqual(new Uint8Array(request.challenge), new Uint8Array([1, 2, 3, 250]))
  assert.deepEqual(new Uint8Array(request.user.id), new Uint8Array([9, 8, 7]))
  assert.equal(request.user.name, 'admin')
  assert.deepEqual(new Uint8Array(request.excludeCredentials[0].id), new Uint8Array([4, 5, 6]))
  assert.deepEqual(request.excludeCredentials[0].transports, ['internal'])
  assert.equal(request.authenticatorSelection.residentKey, 'required')
  assert.deepEqual(registrationRequest({
    challenge, rp: { name: 'Sepia' }, user: { id: handle, name: 'a', displayName: 'a' }, pubKeyCredParams: [],
  }).excludeCredentials, [])
})

test('authenticationRequest decodes challenge and allowCredentials', () => {
  const challenge = bytesToBase64url(new Uint8Array([10, 20]))
  const allowed = bytesToBase64url(new Uint8Array([30, 40, 50]))
  const request = authenticationRequest({
    challenge, rpId: 'localhost', userVerification: 'required',
    allowCredentials: [{ type: 'public-key', id: allowed }],
  })
  assert.deepEqual(new Uint8Array(request.challenge), new Uint8Array([10, 20]))
  assert.deepEqual(new Uint8Array(request.allowCredentials[0].id), new Uint8Array([30, 40, 50]))
  assert.equal(request.rpId, 'localhost')
  assert.deepEqual(authenticationRequest({ challenge }).allowCredentials, [])
})

test('registrationResponse encodes the attestation into gateway wire JSON', () => {
  const rawId = new Uint8Array([1, 2, 3, 4])
  const attestation = new Uint8Array([250, 251, 252])
  const clientData = new TextEncoder().encode('{"type":"webauthn.create"}')
  const credential = {
    id: bytesToBase64url(rawId), rawId: rawId.buffer, type: 'public-key',
    response: {
      attestationObject: attestation.buffer, clientDataJSON: clientData.buffer,
      getTransports: () => ['internal', 'hybrid'],
    },
    getClientExtensionResults: () => ({ credProps: { rk: true } }),
  }
  const wire = registrationResponse(credential)
  assert.equal(wire.id, bytesToBase64url(rawId))
  assert.equal(wire.rawId, bytesToBase64url(rawId))
  assert.equal(wire.type, 'public-key')
  assert.equal(wire.response.attestationObject, bytesToBase64url(attestation))
  assert.equal(wire.response.clientDataJSON, bytesToBase64url(clientData))
  assert.deepEqual(wire.response.transports, ['internal', 'hybrid'])
  assert.deepEqual(wire.clientExtensionResults, { credProps: { rk: true } })
})

test('authenticationResponse encodes signature and keeps a null userHandle', () => {
  const rawId = new Uint8Array([5, 6])
  const authData = new Uint8Array([7, 8, 9])
  const clientData = new TextEncoder().encode('{"type":"webauthn.get"}')
  const signature = new Uint8Array([48, 69, 2, 33])
  const base = {
    id: bytesToBase64url(rawId), rawId: rawId.buffer, type: 'public-key',
    getClientExtensionResults: () => ({}),
  }
  const wire = authenticationResponse({
    ...base,
    response: { authenticatorData: authData.buffer, clientDataJSON: clientData.buffer, signature: signature.buffer, userHandle: null },
  })
  assert.equal(wire.response.signature, bytesToBase64url(signature))
  assert.equal(wire.response.authenticatorData, bytesToBase64url(authData))
  assert.equal(wire.response.userHandle, null)
  const withHandle = authenticationResponse({
    ...base,
    response: { authenticatorData: authData.buffer, clientDataJSON: clientData.buffer, signature: signature.buffer, userHandle: new Uint8Array([11, 12]).buffer },
  })
  assert.equal(withHandle.response.userHandle, bytesToBase64url(new Uint8Array([11, 12])))
})

test('passkeySupported reports false without a browser WebAuthn surface', () => {
  assert.equal(passkeySupported(), false)
})