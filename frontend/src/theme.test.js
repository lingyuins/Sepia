import assert from 'node:assert/strict'
import test from 'node:test'

import { themeTokens, themeSkin, themeMode } from './theme.ts'

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

test('the Octopus skin pins its Scandi mint palette regardless of accent', () => {
  assert.deepEqual(themeTokens('dark', 'Sky', 'octopus'), { accent: '#6ba887', accentText: '#151107', connection: '#6ba887', focus: '#6ba887' })
  assert.deepEqual(themeTokens('light', 'Sand', 'octopus'), { accent: '#599576', accentText: '#f9f8f5', connection: '#599576', focus: '#599576' })
})

test('theme families resolve skin and mode independently of the system preference', () => {
  assert.equal(themeSkin('Octopus dark'), 'octopus')
  assert.equal(themeSkin('Octopus light'), 'octopus')
  assert.equal(themeSkin('Sepia dark'), 'sepia')
  assert.equal(themeSkin('System'), 'sepia')
  assert.equal(themeMode('Octopus dark', true), 'dark')
  assert.equal(themeMode('Octopus light', false), 'light')
  assert.equal(themeMode('System', true), 'light')
  assert.equal(themeMode('Sepia dark', true), 'dark')
})
