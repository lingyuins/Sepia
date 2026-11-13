import assert from 'node:assert/strict'
import test from 'node:test'

import { themeTokens } from './theme.ts'

test('uses the existing accent colors in dark mode', () => {
  assert.deepEqual(themeTokens('dark', 'Sage'), { accent: '#c4eaaa', accentText: '#1a2a1a', connection: '#c4eaaa', focus: '#c4eaaa' })
  assert.deepEqual(themeTokens('dark', 'Sky'), { accent: '#a5cfe5', accentText: '#152a34', connection: '#a5cfe5', focus: '#a5cfe5' })
})

test('uses readable accent variants for every light theme preference', () => {
  assert.deepEqual(themeTokens('light', 'Sage'), { accent: '#426b31', accentText: '#ffffff', connection: '#426b31', focus: '#426b31' })
  assert.deepEqual(themeTokens('light', 'Sky'), { accent: '#2d6078', accentText: '#ffffff', connection: '#2d6078', focus: '#2d6078' })
  assert.deepEqual(themeTokens('light', 'Lavender'), { accent: '#65548e', accentText: '#ffffff', connection: '#65548e', focus: '#65548e' })
  assert.deepEqual(themeTokens('light', 'Sand'), { accent: '#78552b', accentText: '#ffffff', connection: '#78552b', focus: '#78552b' })
})

test('falls back to Sage for an unknown accent preference', () => {
  assert.deepEqual(themeTokens('light', 'unknown'), themeTokens('light', 'Sage'))
})
