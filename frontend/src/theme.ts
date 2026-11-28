export type ThemeMode = 'dark' | 'light'
export type ThemeSkin = 'sepia' | 'octopus'

// The Octopus skin is a theme family of its own: two fixed Scandi palettes
// (warm paper light / warm brown dark) with a mint primary, independent of the
// Sepia accent picker. Everything else keeps the original Sepia behavior.
export function themeSkin(theme: string): ThemeSkin {
  return theme.startsWith('Octopus') ? 'octopus' : 'sepia'
}

export function themeMode(theme: string, systemLight = false): ThemeMode {
  if (theme === 'Octopus light') return 'light'
  if (theme === 'Octopus dark') return 'dark'
  return theme === 'Light' || (theme === 'System' && systemLight) ? 'light' : 'dark'
}

const darkAccents: Record<string, [string, string]> = {
  Sage: ['#c4eaaa', '#1a2a1a'],
  Sky: ['#a5cfe5', '#152a34'],
  Lavender: ['#c3b3e5', '#241d37'],
  Sand: ['#dfc49f', '#322719'],
}
const lightAccents: Record<string, [string, string]> = {
  Sage: ['#426b31', '#ffffff'],
  Sky: ['#2d6078', '#ffffff'],
  Lavender: ['#65548e', '#ffffff'],
  Sand: ['#78552b', '#ffffff'],
}

export function themeTokens(mode: ThemeMode, accent: string, skin: ThemeSkin = 'sepia') {
  if (skin === 'octopus') {
    const [accentColor, accentText] = mode === 'light' ? ['#599576', '#f9f8f5'] : ['#6ba887', '#151107']
    return { accent: accentColor, accentText, connection: accentColor, focus: accentColor }
  }
  const [accentColor, accentText] = (mode === 'light' ? lightAccents : darkAccents)[accent] || (mode === 'light' ? lightAccents : darkAccents).Sage
  return { accent: accentColor, accentText, connection: accentColor, focus: accentColor }
}

export function applyTheme(root: HTMLElement, settings: { theme: string; accent: string; reducedMotion: boolean; compact: boolean; language: string }, systemLight = false) {
  const skin = themeSkin(settings.theme)
  const mode = themeMode(settings.theme, systemLight)
  const tokens = themeTokens(mode, settings.accent, skin)
  root.dataset.theme = mode
  root.dataset.skin = skin
  root.dataset.accent = settings.accent in darkAccents ? settings.accent : 'Sage'
  root.style.setProperty('--accent', tokens.accent)
  root.style.setProperty('--accent-text', tokens.accentText)
  root.style.setProperty('--focus', tokens.focus)
  root.style.setProperty('--connection', tokens.connection)
  root.classList.toggle('reduce-motion', settings.reducedMotion)
  root.classList.toggle('compact-tables', settings.compact)
  root.lang = settings.language === '简体中文' ? 'zh-CN' : settings.language === '日本語' ? 'ja' : 'en'
  // Keep the PWA/browser chrome in step with the console surface color.
  const chrome = skin === 'octopus'
    ? (mode === 'light' ? '#f9f8f5' : '#1c1a15')
    : (mode === 'light' ? '#f5f7f6' : '#0c1113')
  root.ownerDocument.querySelector('meta[name="theme-color"]')?.setAttribute('content', chrome)
}
