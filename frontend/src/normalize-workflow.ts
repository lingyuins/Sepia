// Offline AI normalization workflow helpers (ported from octopus
// web/src/components/modules/setting/Normalize.tsx). Pure logic: the editor
// exports a names-only corpus, an external AI returns normalize-rules.json,
// and parseRulesFile turns that file into a full replacement candidate.
// Applying the candidate only replaces editor state; saving stays explicit.

import type { ExplicitMapping } from './model-normalize'

export type WorkflowRules = { routerPrefixes: string[]; functionalSuffixes: string[]; explicitMappings: ExplicitMapping[] }

export type RulesFileResult = { ok: true; rules: WorkflowRules } | { ok: false; error: 'shape' | 'arrays' | 'entries' }

// Unique, trimmed, sorted model names — the corpus an offline AI analyzes.
export function buildModelNamesExport(names: string[], now: Date = new Date()): { exported_at: string; model_names: string[] } {
  const seen = new Set<string>()
  for (const name of names) {
    const trimmed = name.trim()
    if (trimmed) seen.add(trimmed)
  }
  return { exported_at: now.toISOString(), model_names: [...seen].sort() }
}

export function modelChannelsFilename(now: Date = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, '0')
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  return `model-channels-${stamp}.json`
}

// Strict octopus contract: exactly the three rule arrays, every entry typed.
// Any invalid entry rejects the whole file instead of silently dropping it.
export function parseRulesFile(text: string): RulesFileResult {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch {
    return { ok: false, error: 'shape' }
  }
  if (!data || typeof data !== 'object' || Array.isArray(data)) return { ok: false, error: 'shape' }
  const record = data as Record<string, unknown>
  if (Object.keys(record).sort().join('|') !== 'explicit_mappings|functional_suffixes|router_prefixes') return { ok: false, error: 'shape' }
  const { router_prefixes: prefixes, functional_suffixes: suffixes, explicit_mappings: mappings } = record
  if (!Array.isArray(prefixes) || !Array.isArray(suffixes) || !Array.isArray(mappings)) return { ok: false, error: 'arrays' }
  const routerPrefixes: string[] = []
  for (const entry of prefixes) {
    if (typeof entry !== 'string' || !entry.trim()) return { ok: false, error: 'entries' }
    const trimmed = entry.trim()
    // Regex prefixes must keep their pattern intact; literal prefixes get the
    // trailing separator the engine expects.
    routerPrefixes.push(/[\\{}*+?|]/.test(trimmed) || /[-/.:_\])\]]$/.test(trimmed) ? trimmed : `${trimmed}-`)
  }
  const functionalSuffixes: string[] = []
  for (const entry of suffixes) {
    if (typeof entry !== 'string' || !entry.trim()) return { ok: false, error: 'entries' }
    const trimmed = entry.trim()
    functionalSuffixes.push(trimmed.startsWith('-') ? trimmed : `-${trimmed}`)
  }
  const explicitMappings: ExplicitMapping[] = []
  for (const entry of mappings) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) return { ok: false, error: 'entries' }
    const { variant, canonical } = entry as Record<string, unknown>
    if (typeof variant !== 'string' || typeof canonical !== 'string' || !variant.trim() || !canonical.trim()) return { ok: false, error: 'entries' }
    explicitMappings.push({ variant: variant.trim(), canonical: canonical.trim().toLowerCase() })
  }
  const seenMappings = new Set<string>()
  const uniqueMappings = explicitMappings.filter((mapping) => {
    const key = `${mapping.variant}\u0000${mapping.canonical}`
    if (seenMappings.has(key)) return false
    seenMappings.add(key)
    return true
  })
  return { ok: true, rules: { routerPrefixes: [...new Set(routerPrefixes)], functionalSuffixes: [...new Set(functionalSuffixes)], explicitMappings: uniqueMappings } }
}

// Coverage of a candidate rule set over the current corpus, for the import
// preview: how many names normalize, how many distinct canonical names remain
// and how many variants actually merged.
export function candidateCoverage(names: string[], normalize: (name: string) => string): { input: number; canonical: number; merged: number } {
  const bases = new Map<string, number>()
  let input = 0
  for (const name of names) {
    const trimmed = name.trim()
    if (!trimmed) continue
    input += 1
    const base = normalize(trimmed)
    bases.set(base, (bases.get(base) ?? 0) + 1)
  }
  let merged = 0
  for (const count of bases.values()) if (count > 1) merged += count - 1
  return { input, canonical: bases.size, merged }
}