import type { Language, TranslationKey } from './i18n.ts'

export function localeOf(language: string) {
  return language === '简体中文' ? 'zh-CN' : language === '日本語' ? 'ja-JP' : 'en-US'
}

export function translate(
  dictionary: Record<TranslationKey, readonly string[]>,
  language: Language,
  key: TranslationKey,
  values?: Record<string, string | number>,
) {
  const languageIndex = language === '简体中文' ? 1 : language === '日本語' ? 2 : 0
  let text = dictionary[key]?.[languageIndex] || dictionary[key]?.[0] || key
  if (values) {
    for (const [name, value] of Object.entries(values)) {
      text = text.split(`{${name}}`).join(String(value))
    }
  }
  return text
}
