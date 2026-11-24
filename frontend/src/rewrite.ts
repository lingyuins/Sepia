import type { TranslationKey } from './i18n'

export type HeaderRule = { name: string; value: string }
export type ParamReplaceRule = { field: string; pattern: string; replacement: string }
export type PromptReplaceRule = { pattern: string; replacement: string }
export type ParamDefaultRule = { field: string; value: unknown; text?: string }
export type RewriteConfig = {
  headers: HeaderRule[]
  paramReplace: ParamReplaceRule[]
  promptReplace: PromptReplaceRule[]
  paramDefaults: ParamDefaultRule[]
}
export type RewriteSource = Partial<RewriteConfig> | null | undefined
export type RewriteIssue = { key: TranslationKey; values?: Record<string, string> }

// Headers the gateway owns per request: overriding them would break
// authentication, framing or routing, so they are rejected at save time.
export const RESERVED_HEADERS = [
  'authorization',
  'proxy-authorization',
  'cookie',
  'set-cookie',
  'host',
  'x-api-key',
  'x-goog-api-key',
  'anthropic-version',
  'content-length',
  'transfer-encoding',
  'connection',
  'keep-alive',
]

export function rewriteOf(source: RewriteSource): RewriteConfig {
  return {
    headers: (source?.headers ?? []).map((rule) => ({ name: rule.name, value: rule.value })),
    paramReplace: (source?.paramReplace ?? []).map((rule) => ({ field: rule.field, pattern: rule.pattern, replacement: rule.replacement })),
    promptReplace: (source?.promptReplace ?? []).map((rule) => ({ pattern: rule.pattern, replacement: rule.replacement })),
    paramDefaults: (source?.paramDefaults ?? []).map((rule) => ({ ...rule })),
  }
}

export function rewriteCount(config: RewriteSource): number {
  const rules = rewriteOf(config)
  return rules.headers.length + rules.paramReplace.length + rules.promptReplace.length + rules.paramDefaults.length
}

// Later layer wins per key; nothing is concatenated. Header names compare
// case-insensitively, everything else by exact key.
function mergeBy<T>(base: T[], override: T[], keyOf: (item: T) => string): T[] {
  const merged = new Map<string, T>()
  for (const rule of base) merged.set(keyOf(rule), rule)
  for (const rule of override) merged.set(keyOf(rule), rule)
  return [...merged.values()]
}

export function mergeRewrite(provider: RewriteSource, route: RewriteSource): RewriteConfig {
  const base = rewriteOf(provider)
  const top = rewriteOf(route)
  return {
    headers: mergeBy(base.headers, top.headers, (rule) => rule.name.trim().toLowerCase()),
    paramReplace: mergeBy(base.paramReplace, top.paramReplace, (rule) => rule.field.trim()),
    promptReplace: mergeBy(base.promptReplace, top.promptReplace, (rule) => rule.pattern),
    paramDefaults: mergeBy(base.paramDefaults, top.paramDefaults, (rule) => rule.field.trim()),
  }
}

export function isReservedHeader(name: string) {
  return RESERVED_HEADERS.includes(name.trim().toLowerCase())
}

export function testRegex(pattern: string, replacement: string, sample: string) {
  // The gateway substitutes ${1} and ${name}; browsers only understand $1 and
  // $<name>, so translate before previewing to mirror the server result.
  const browser = replacement.replace(/\$\{(\d+)\}/g, '$$$1').replace(/\$\{(\w+)\}/g, '$<$1>')
  return sample.replace(new RegExp(pattern, 'g'), browser)
}

// Parameter defaults are arbitrary JSON values edited as text. The draft keeps
// the raw text next to the parsed value so a half-typed object survives
// re-renders; the wire payload sends the parsed value only.
export function parseParamValue(text: string): unknown {
  const trimmed = text.trim()
  if (!trimmed) return undefined
  try {
    return JSON.parse(trimmed)
  } catch {
    return undefined
  }
}

export function formatParamValue(value: unknown): string {
  if (value === undefined) return ''
  const encoded = JSON.stringify(value)
  return encoded === undefined ? '' : encoded
}

function compileIssue(pattern: string): RewriteIssue | null {
  if (!pattern.trim()) return { key: 'rewritePatternRequired' }
  try {
    new RegExp(pattern)
    return null
  } catch {
    return { key: 'rewriteInvalidPattern', values: { pattern } }
  }
}

export function validateRewrite(config: RewriteSource): RewriteIssue[] {
  const rules = rewriteOf(config)
  const issues: RewriteIssue[] = []
  for (const rule of rules.headers) {
    const name = rule.name.trim()
    if (!name) {
      issues.push({ key: 'rewriteHeaderNameRequired' })
    } else if (rule.name !== name) {
      issues.push({ key: 'rewriteHeaderNameSpaces', values: { name: rule.name } })
    } else if (isReservedHeader(name)) {
      issues.push({ key: 'rewriteHeaderReserved', values: { name } })
    }
    if (rule.value.includes('\r') || rule.value.includes('\n')) {
      issues.push({ key: 'rewriteHeaderValueLine', values: { name } })
    }
  }
  for (const rule of rules.paramReplace) {
    if (!rule.field.trim()) issues.push({ key: 'rewriteFieldRequired' })
    const issue = compileIssue(rule.pattern)
    if (issue) issues.push(issue)
  }
  for (const rule of rules.promptReplace) {
    const issue = compileIssue(rule.pattern)
    if (issue) issues.push(issue)
  }
  for (const rule of rules.paramDefaults) {
    if (!rule.field.trim()) issues.push({ key: 'rewriteFieldRequired' })
    if (rule.value === undefined) issues.push({ key: 'rewriteDefaultInvalid', values: { field: rule.field } })
  }
  return issues
}
