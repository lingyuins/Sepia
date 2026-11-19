// Pure WebAuthn JSON <-> browser credential mapping (no DOM at import time).
// The gateway speaks base64url without padding; navigator.credentials wants ArrayBuffers.
export function base64urlToBytes(value: string): Uint8Array {
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4)
  const binary = atob(padded)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

export function bytesToBase64url(buffer: ArrayBuffer | Uint8Array): string {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer)
  let binary = ''
  for (let index = 0; index < bytes.length; index += 1) binary += String.fromCharCode(bytes[index])
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

type RegisterOptions = {
  challenge: string
  rp: { id?: string; name: string }
  user: { id: string; name: string; displayName: string }
  pubKeyCredParams: { type: string; alg: number }[]
  timeout?: number
  excludeCredentials?: { type: string; id: string; transports?: string[] }[]
  authenticatorSelection?: Record<string, unknown>
  attestation?: string
  extensions?: Record<string, unknown>
}

type RequestOptions = {
  challenge: string
  rpId?: string
  timeout?: number
  allowCredentials?: { type: string; id: string; transports?: string[] }[]
  userVerification?: string
  extensions?: Record<string, unknown>
}

export function registrationRequest(options: RegisterOptions): PublicKeyCredentialCreationOptions {
  return {
    ...options,
    challenge: base64urlToBytes(options.challenge),
    user: { ...options.user, id: base64urlToBytes(options.user.id) },
    excludeCredentials: (options.excludeCredentials || []).map((credential) => ({ ...credential, id: base64urlToBytes(credential.id) })),
  } as PublicKeyCredentialCreationOptions
}

export function authenticationRequest(options: RequestOptions): PublicKeyCredentialRequestOptions {
  return {
    ...options,
    challenge: base64urlToBytes(options.challenge),
    allowCredentials: (options.allowCredentials || []).map((credential) => ({ ...credential, id: base64urlToBytes(credential.id) })),
  } as PublicKeyCredentialRequestOptions
}

export function registrationResponse(credential: PublicKeyCredential): Record<string, unknown> {
  const response = credential.response as AuthenticatorAttestationResponse
  return {
    id: credential.id,
    rawId: bytesToBase64url(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: bytesToBase64url(response.attestationObject),
      clientDataJSON: bytesToBase64url(response.clientDataJSON),
      transports: typeof response.getTransports === 'function' ? response.getTransports() : [],
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  }
}

export function authenticationResponse(credential: PublicKeyCredential): Record<string, unknown> {
  const response = credential.response as AuthenticatorAssertionResponse
  return {
    id: credential.id,
    rawId: bytesToBase64url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: bytesToBase64url(response.authenticatorData),
      clientDataJSON: bytesToBase64url(response.clientDataJSON),
      signature: bytesToBase64url(response.signature),
      userHandle: response.userHandle ? bytesToBase64url(response.userHandle) : null,
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  }
}

export function passkeySupported(): boolean {
  return typeof window !== 'undefined' && typeof window.PublicKeyCredential !== 'undefined' && typeof navigator.credentials?.create === 'function'
}