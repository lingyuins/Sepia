import assert from 'node:assert/strict'
import test from 'node:test'

import { completeOnboarding, isOnboardingComplete } from './onboarding.ts'

test('requires onboarding before the saved completion marker exists', () => {
  assert.equal(isOnboardingComplete(null), false)
  assert.equal(isOnboardingComplete('invalid'), false)
})

test('recognizes the saved onboarding completion marker', () => {
  assert.equal(isOnboardingComplete('complete'), true)
})

test('finishing onboarding keeps existing preferences and trims the workspace name', () => {
  const settings = { workspace: 'Personal workspace', language: 'English', theme: 'Light', accent: 'Sage' }
  const result = completeOnboarding(settings, { workspace: '  Team gateway  ', language: '简体中文', deployment: 'cloud', protocol: 'https', host: ' gateway.example.com ', port: '443' })
  assert.deepEqual(result, { workspace: 'Team gateway', language: '简体中文', theme: 'Light', accent: 'Sage', deployment: 'cloud', protocol: 'https', host: 'gateway.example.com', port: '443' })
})

test('finishing local onboarding always uses HTTP', () => {
  const result = completeOnboarding({ workspace: 'Personal workspace' }, { workspace: 'Local gateway', language: 'English', deployment: 'local', protocol: 'https', host: '127.0.0.1', port: '3425' })
  assert.equal(result.protocol, 'http')
})
