export type ThemeMode = 'dark' | 'light'

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

export function themeTokens(mode: ThemeMode, accent: string) {
  const [accentColor, accentText] = (mode === 'light' ? lightAccents : darkAccents)[accent] || (mode === 'light' ? lightAccents : darkAccents).Sage
  return { accent: accentColor, accentText }
}

export function applyTheme(root: HTMLElement, settings: { theme: string; accent: string; reducedMotion: boolean; compact: boolean; language: string }, systemLight = false) {
  const mode: ThemeMode = settings.theme === 'Light' || (settings.theme === 'System' && systemLight) ? 'light' : 'dark'
  const tokens = themeTokens(mode, settings.accent)
  root.dataset.theme = mode
  root.dataset.accent = settings.accent in darkAccents ? settings.accent : 'Sage'
  root.style.setProperty('--accent', tokens.accent)
  root.style.setProperty('--accent-text', tokens.accentText)
  root.classList.toggle('reduce-motion', settings.reducedMotion)
  root.classList.toggle('compact-tables', settings.compact)
  root.lang = settings.language === '简体中文' ? 'zh-CN' : settings.language === '日本語' ? 'ja' : 'en'
}
