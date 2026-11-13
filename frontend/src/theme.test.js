import assert from 'node:assert/strict'
import test from 'node:test'

import { themeTokens } from './theme.ts'

test('uses the existing accent colors in dark mode', () => {
  assert.deepEqual(themeTokens('dark', 'Sage'), { accent: '#c4eaaa', accentText: '#1a2a1a' })
  assert.deepEqual(themeTokens('dark', 'Sky'), { accent: '#a5cfe5', accentText: '#152a34' })
})

test('uses readable accent variants for every light theme preference', () => {
  assert.deepEqual(themeTokens('light', 'Sage'), { accent: '#426b31', accentText: '#ffffff' })
  assert.deepEqual(themeTokens('light', 'Sky'), { accent: '#2d6078', accentText: '#ffffff' })
  assert.deepEqual(themeTokens('light', 'Lavender'), { accent: '#65548e', accentText: '#ffffff' })
  assert.deepEqual(themeTokens('light', 'Sand'), { accent: '#78552b', accentText: '#ffffff' })
})

test('falls back to Sage for an unknown accent preference', () => {
  assert.deepEqual(themeTokens('light', 'unknown'), themeTokens('light', 'Sage'))
})
