// Model-name normalization, ported from octopus
// (web/src/components/modules/model/normalize.ts + internal/op/modelnormalize).
//
// The same base model appears under many channel-specific names, e.g.
//   kimi-k2.5, @cf/moonshotai/kimi-k2.5, dmxapi-kimi-k2.5, kimi-k2.5-cc
// all normalize to `kimi-k2.5` for plaza family grouping. Normalization only
// affects display grouping; the original model ids are never rewritten.
//
// Rule layers (highest priority first):
//   1. explicit mappings (variant → canonical), matched on dot-dash keys of
//      the router-base and full-base forms;
//   2. user rules injected from gateway settings;
//   3. builtin defaults (empty/absent prefix/suffix lists fall back to them).
//
// Rules are passed explicitly (no module-level mutable state) so the module
// stays pure logic testable with node:test.

export type ExplicitMapping = { variant: string; canonical: string }

export type NormalizeRulesInput = {
  routerPrefixes?: string[] | null
  functionalSuffixes?: string[] | null
  explicitMappings?: ExplicitMapping[] | null
}

export const BUILTIN_ROUTER_PREFIXES = [
  'dmxapi-', 'dmxapl-', 'agent-', 'openai-', 'anthropic-',
  'aihub-', 'aihubmix-', 'ahm-', 'bai-', 'auto-',
  'group-', 'copilot-', 'kilo-auto-', 'cc-', 'cloudflare-',
  'deepinfra-', 'siliconflow-', 'nvidia-', 'zai-', 'xiaomi-',
  'mm-',
  // Channel tags such as [官B]claude-opus-4-6 or [满血A]-gpt-5.
  '\\[[^\\]]+\\]-?',
]

export const BUILTIN_FUNCTIONAL_SUFFIXES = [
  '-cc', '-free', '-:free', '-(free)', '-vip', '-ssvip', '-latest',
  // Official-channel marker, batch tier, discount and dated snapshots.
  '-guan', '-all', '-:batch', '-\\d+off', '-\\d{8}', '-\\d{4}-\\d{2}-\\d{2}',
]

// Tokens that identify model families, capabilities or execution modes.
// A cleanup suffix containing one of them would merge distinct models
// (gpt-4o-mini into gpt-4o), so validation rejects such suffixes.
export const PROTECTED_MODEL_TOKENS = [
  'air', 'audio', 'base', 'chat', 'coder',
  'embed', 'embedding', 'fast', 'flash', 'image',
  'instruct', 'large', 'lite', 'max', 'medium',
  'mini', 'micro', 'nano', 'non-reasoning', 'omni', 'plus',
  'pro', 'reasoning', 'rerank', 'reranker', 'search',
  'searching', 'small', 'think', 'thinking', 'tiny',
  'turbo', 'tts', 'video', 'vision', 'vl', 'big',
  'preview', 'beta', '8k', '16k', '32k', '64k',
  '128k', '192k', '256k', '512k',
]

export type CompiledRules = {
  routerPrefixes: string[]
  functionalSuffixes: string[]
  explicitFullByKey: Map<string, string>
  explicitBaseByKey: Map<string, string>
}

// dotDashKey unifies '-' and '.' and lowercases, so claude-opus-4-6 and
// claude-opus-4.6 share a key while gemini-2-5-pro and gemini-25-pro do not.
export function dotDashKey(value: string): string {
  return value.toLowerCase().trim().replace(/-/g, '.')
}

export function compileNormalizeRules(rules: NormalizeRulesInput = {}): CompiledRules {
  const compiled: CompiledRules = {
    routerPrefixes: rules.routerPrefixes && rules.routerPrefixes.length > 0 ? rules.routerPrefixes : BUILTIN_ROUTER_PREFIXES,
    functionalSuffixes: rules.functionalSuffixes && rules.functionalSuffixes.length > 0 ? rules.functionalSuffixes : BUILTIN_FUNCTIONAL_SUFFIXES,
    explicitFullByKey: new Map(),
    explicitBaseByKey: new Map(),
  }
  for (const mapping of rules.explicitMappings ?? []) {
    const variant = mapping.variant.trim()
    const canonical = mapping.canonical.trim().toLowerCase()
    if (!variant || !canonical) continue
    const fullKey = dotDashKey(stripPathAndRouterPrefix(variant, compiled.routerPrefixes))
    if (fullKey && !compiled.explicitFullByKey.has(fullKey)) compiled.explicitFullByKey.set(fullKey, canonical)
    // Suffixes such as -cc never match after dot-dashing turns '-' into '.', so
    // strip from the plain path-stripped form and dot-dash afterwards, exactly
    // like the lookup path in normalizeModelName.
    const baseKey = dotDashKey(stripFunctionalSuffixes(stripPathAndRouterPrefix(variant, compiled.routerPrefixes), compiled.functionalSuffixes))
    if (baseKey && !compiled.explicitBaseByKey.has(baseKey)) compiled.explicitBaseByKey.set(baseKey, canonical)
  }
  return compiled
}

const DEFAULT_RULES = compileNormalizeRules()

export function normalizeModelName(name: string, rules: CompiledRules = DEFAULT_RULES): string {
  if (!name) return ''
  const trimmed = name.trim()
  const routerBase = stripPathAndRouterPrefix(trimmed, rules.routerPrefixes).toLowerCase()
  if (rules.explicitFullByKey.size > 0 || rules.explicitBaseByKey.size > 0) {
    const fullCanonical = rules.explicitFullByKey.get(dotDashKey(routerBase))
    if (fullCanonical) return fullCanonical
    const base = stripFunctionalSuffixes(routerBase, rules.functionalSuffixes).toLowerCase()
    const baseCanonical = rules.explicitBaseByKey.get(dotDashKey(base))
    if (baseCanonical) return baseCanonical
  }
  const result = stripFunctionalSuffixes(routerBase, rules.functionalSuffixes).trim().toLowerCase()
  // Malformed path-like inputs must not disappear from the plaza.
  return result || trimmed.toLowerCase()
}

// stripPathAndRouterPrefix keeps the segment after the last '/', then removes
// one known router prefix (case-insensitive). A trailing-'-' prefix such as
// '[官B]-' also matches without the hyphen ('[官B]claude-opus-4-6') when the
// prefix base ends in a non-alphanumeric character.
function stripPathAndRouterPrefix(name: string, prefixes: string[]): string {
  let result = name
  const slashIndex = result.lastIndexOf('/')
  if (slashIndex >= 0) result = result.slice(slashIndex + 1)
  const lower = result.toLowerCase()
  for (const prefix of prefixes) {
    const p = prefix.trim()
    if (!p) continue
    if (isRegexSuffix(p)) {
      const re = getPrefixRegex(p)
      const match = re?.exec(lower)
      if (match && match[0].length > 0 && match[0].length < result.length) {
        result = result.slice(match[0].length)
        break
      }
      continue
    }
    if (lower.startsWith(p.toLowerCase())) {
      result = result.slice(p.length)
      break
    }
    if (p.endsWith('-')) {
      const base = p.slice(0, -1)
      if (base && !endsWithAlphanumeric(base) && lower.startsWith(base.toLowerCase())) {
        result = result.slice(base.length).replace(/^-+/, '')
        break
      }
    }
  }
  return result
}

function endsWithAlphanumeric(value: string): boolean {
  const characters = Array.from(value)
  if (characters.length === 0) return false
  return /^[\p{L}\p{N}]$/u.test(characters[characters.length - 1])
}

// stripFunctionalSuffixes loops until no suffix matches. Literal candidates:
// as-is, '-:' → ':' and '-(' → '(' forms. Regex candidates (only when the
// suffix contains regex metacharacters): as-is and '-@' → '@' form, anchored
// at the end, case-insensitive, with an empty-match guard so nullable
// patterns cannot loop forever.
function stripFunctionalSuffixes(name: string, suffixes: string[]): string {
  let result = name
  let changed = true
  while (changed) {
    changed = false
    const lower = result.toLowerCase()
    for (const suffix of suffixes) {
      const s = suffix.trim()
      if (!s) continue
      const n = matchSuffixCandidate(lower, s)
      if (n > 0 && result.length > n) {
        result = result.slice(0, -n)
        changed = true
        break
      }
    }
  }
  return result
}

const suffixRegexCache = new Map<string, RegExp | null>()

function matchSuffixCandidate(lower: string, suffix: string): number {
  const literal = [suffix.toLowerCase()]
  if (suffix.startsWith('-:')) literal.push(suffix.slice(1).toLowerCase())
  // '-(free)' strips both the hyphen and the closing parenthesis.
  if (suffix.startsWith('-(') && suffix.endsWith(')')) literal.push(suffix.slice(1).toLowerCase())
  for (const candidate of literal) {
    if (candidate && lower.endsWith(candidate) && lower.length > candidate.length) return candidate.length
  }
  if (isRegexSuffix(suffix)) {
    const patterns = [suffix]
    if (suffix.startsWith('-@')) patterns.push(suffix.slice(1))
    for (const pattern of patterns) {
      const re = getSuffixRegex(pattern)
      if (!re) continue
      const match = re.exec(lower)
      if (match && match.index < lower.length) return lower.length - match.index
    }
  }
  return 0
}

function getSuffixRegex(pattern: string): RegExp | null {
  const cached = suffixRegexCache.get(pattern)
  if (cached !== undefined) return cached
  let re: RegExp | null = null
  try {
    // Wrap in a group so every '|' branch is anchored at the end.
    re = new RegExp('(?:' + pattern + ')$', 'i')
  } catch {
    re = null
  }
  suffixRegexCache.set(pattern, re)
  return re
}

const prefixRegexCache = new Map<string, RegExp | null>()

function getPrefixRegex(pattern: string): RegExp | null {
  const cached = prefixRegexCache.get(pattern)
  if (cached !== undefined) return cached
  let re: RegExp | null = null
  try {
    re = new RegExp('^(?:' + pattern + ')', 'i')
  } catch {
    re = null
  }
  prefixRegexCache.set(pattern, re)
  return re
}

// A rule is an explicit regex only when it contains regex metacharacters
// (backslash, { } * + ? |). Bare letters stay literal: 'dmxapi-' must not be
// treated as the pattern 'd|w' just because it contains d or w.
function isRegexSuffix(suffix: string): boolean {
  return /[\\{}*+?|]/.test(suffix)
}

export type RuleError = { key: string; params?: Record<string, string | number> }

// validateNormalizeRules mirrors octopus modelnormalize.ValidateRules.
// Returns the first violation as an i18n key, or null when the rules are safe
// to persist. The runtime compiler stays tolerant of legacy malformed rules.
export function validateNormalizeRules(rules: NormalizeRulesInput): RuleError | null {
  const prefixes = rules.routerPrefixes ?? []
  const seenPrefixes = new Set<string>()
  for (let index = 0; index < prefixes.length; index++) {
    const prefix = prefixes[index].trim()
    if (!prefix) return { key: 'normalizeErrPrefixEmpty', params: { index: index + 1 } }
    if (prefix.includes('/')) return { key: 'normalizeErrPrefixPath', params: { value: prefix } }
    if (isRegexSuffix(prefix)) {
      if (!getPrefixRegex(prefix)) return { key: 'normalizeErrPrefixRegex', params: { value: prefix } }
    } else if (!'-/.:_])'.includes(prefix[prefix.length - 1])) {
      return { key: 'normalizeErrPrefixBoundary', params: { value: prefix } }
    }
    const lower = prefix.toLowerCase()
    if (seenPrefixes.has(lower)) return { key: 'normalizeErrPrefixDuplicate', params: { value: prefix } }
    seenPrefixes.add(lower)
  }
  const suffixes = rules.functionalSuffixes ?? []
  for (let index = 0; index < suffixes.length; index++) {
    const suffix = suffixes[index].trim()
    if (!suffix) return { key: 'normalizeErrSuffixEmpty', params: { index: index + 1 } }
    if (/[/\r\n\t]/.test(suffix)) return { key: 'normalizeErrSuffixPath', params: { value: suffix } }
    const lower = suffix.toLowerCase()
    if (lower.includes('\\d') && /[km]/.test(lower)) return { key: 'normalizeErrSuffixContext', params: { value: suffix } }
    if (isRegexSuffix(suffix) && !getSuffixRegex(suffix)) return { key: 'normalizeErrSuffixRegex', params: { value: suffix } }
    const fragment = suffixSemanticFragment(suffix)
    for (const token of fragment.toLowerCase().split(/[-._:@()[\]]+/)) {
      if (token && PROTECTED_MODEL_TOKENS.includes(token)) return { key: 'normalizeErrSuffixProtected', params: { value: suffix, token } }
    }
  }
  const compiled = compileNormalizeRules(rules)
  const mappings = rules.explicitMappings ?? []
  const mappingsByKey = new Map<string, string>()
  for (let index = 0; index < mappings.length; index++) {
    const variant = mappings[index].variant.trim()
    const canonical = mappings[index].canonical.trim().toLowerCase()
    if (!variant || !canonical) return { key: 'normalizeErrMappingEmpty', params: { index: index + 1 } }
    if (/[/[\]\s]/.test(canonical)) return { key: 'normalizeErrMappingCanonical', params: { index: index + 1, value: canonical } }
    const withoutExplicit = stripFunctionalSuffixes(stripPathAndRouterPrefix(canonical, compiled.routerPrefixes), compiled.functionalSuffixes).trim().toLowerCase()
    if (withoutExplicit !== canonical) return { key: 'normalizeErrMappingUnstable', params: { index: index + 1, value: canonical } }
    const routerBase = stripPathAndRouterPrefix(variant, compiled.routerPrefixes)
    for (const key of [dotDashKey(routerBase), dotDashKey(stripFunctionalSuffixes(routerBase, compiled.functionalSuffixes))]) {
      if (!key) return { key: 'normalizeErrMappingEmpty', params: { index: index + 1 } }
      const previous = mappingsByKey.get(key)
      if (previous !== undefined && previous !== canonical) return { key: 'normalizeErrMappingConflict', params: { value: key, previous, canonical } }
      mappingsByKey.set(key, canonical)
    }
  }
  return null
}

function suffixSemanticFragment(value: string): string {
  let fragment = value.trim().replace(/^-+/, '')
  if (isRegexSuffix(fragment)) {
    fragment = fragment
      .replace(/\\d/g, '').replace(/\\w/g, '')
      .replace(/[{}+*?]/g, '')
      .replace(/\|/g, '-')
  }
  return fragment
}