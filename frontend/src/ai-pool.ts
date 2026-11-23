import type { RouteCandidate, RouteRule } from './workspace'
import { normalizeModelName, type CompiledRules } from './model-normalize.ts'

export type AiPoolService = {
  id: number
  name: string
  providerId: number | null
  endpoint: string
  model: string
  models: string[]
  timeoutSeconds: number
  concurrency: number
  enabled: boolean
}

export type AiPoolServiceDraft = {
  name: string
  providerId: number | null
  endpoint: string
  secret: string
  model: string
  models: string[]
  timeoutSeconds: string
  concurrency: string
  enabled: boolean
}

export const emptyAiServiceDraft: AiPoolServiceDraft = { name: '', providerId: null, endpoint: '', secret: '', model: '', models: [], timeoutSeconds: '60', concurrency: '2', enabled: true }

export type AiServiceDraftError = { key: string; params?: Record<string, string> }

// Mirrors backend validate(): name/endpoint/model required, timeout 1..600s,
// concurrency 1..16. A blank secret is allowed when editing (keeps stored key).
export function validateAiServiceDraft(draft: AiPoolServiceDraft, editing: boolean): AiServiceDraftError | null {
  if (!draft.name.trim()) return { key: 'aiServiceNameRequired' }
  // Provider-backed services borrow the provider's address and key, so the
  // endpoint/secret fields are not required (mirrors backend validate()).
  if (draft.providerId === null) {
    const endpoint = draft.endpoint.trim()
    let parsed: URL
    try { parsed = new URL(endpoint) } catch { return { key: 'aiServiceEndpointInvalid' } }
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return { key: 'aiServiceEndpointInvalid' }
    if (!editing && !draft.secret.trim()) return { key: 'aiServiceKeyRequired' }
  }
  if (!draft.model.trim()) return { key: 'aiServiceModelRequired' }
  const timeout = Number(draft.timeoutSeconds)
  if (!Number.isInteger(timeout) || timeout < 1 || timeout > 600) return { key: 'aiServiceTimeoutInvalid' }
  const concurrency = Number(draft.concurrency)
  if (!Number.isInteger(concurrency) || concurrency < 1 || concurrency > 16) return { key: 'aiServiceConcurrencyInvalid' }
  return null
}

export function aiServiceInput(draft: AiPoolServiceDraft) {
  return {
    name: draft.name.trim(),
    provider_id: draft.providerId,
    endpoint: draft.endpoint.trim(),
    secret: draft.secret.trim(),
    model: draft.model.trim(),
    models: draft.models,
    timeout_seconds: Number(draft.timeoutSeconds),
    concurrency: Number(draft.concurrency),
    enabled: draft.enabled,
  }
}

export type ApiAiPoolService = { id: number; name: string; provider_id: number | null; endpoint: string; model: string; models: string[]; timeout_seconds: number; concurrency: number; enabled: boolean; created_at: string; updated_at: string }

export function mapAiPoolService(service: ApiAiPoolService): AiPoolService {
  return { id: service.id, name: service.name, providerId: service.provider_id ?? null, endpoint: service.endpoint, model: service.model, models: service.models, timeoutSeconds: service.timeout_seconds, concurrency: service.concurrency, enabled: service.enabled }
}

export function serviceDraftFrom(service: AiPoolService): AiPoolServiceDraft {
  return { name: service.name, providerId: service.providerId, endpoint: service.endpoint, secret: '', model: service.model, models: service.models, timeoutSeconds: String(service.timeoutSeconds), concurrency: String(service.concurrency), enabled: service.enabled }
}

export type FamilyHint = { provider_id: number; model: string; family: string }

// Family hints for the AI route generator. The frontend owns the configurable
// normalization rules, so families computed here are the single source of
// truth; the backend falls back to metadata::base_model for unhinted entries.
// Normalizer rules strip router prefixes, functional suffixes and dated/free
// forms, so raw model ids are passed straight through.
export function buildFamilyHints(
  providers: { id: number; enabled: boolean; modelsList: string[]; keyCount?: number }[],
  rules: CompiledRules,
): FamilyHint[] {
  const hints: FamilyHint[] = []
  for (const provider of providers) {
    if (!provider.enabled || (provider.keyCount !== undefined && provider.keyCount < 1)) continue
    for (const model of provider.modelsList) {
      const family = normalizeModelName(model, rules).trim().toLowerCase().slice(0, 128)
      if (family) hints.push({ provider_id: provider.id, model, family })
    }
  }
  return hints
}

export type GeneratedRoute = {
  slug: string
  name: string
  description: string
  patterns: string[]
  candidates: { provider_id: number; provider: string; model: string }[]
}

export type GenerateResponse = { routes: GeneratedRoute[]; errors: string[]; batches: number; services: number }

export function generatedRouteRule(route: GeneratedRoute, index: number): RouteRule {
  const candidates: RouteCandidate[] = route.candidates.map((candidate, position) => ({ id: `ai-${index}-${position}`, provider: candidate.provider, model: candidate.model, enabled: true, weight: 1 }))
  const patterns = route.patterns.length ? route.patterns : [route.slug]
  return {
    id: Date.now() + index,
    name: route.name || route.slug,
    slug: route.slug,
    description: route.description,
    match: patterns[0],
    strategy: 'Failover',
    active: true,
    requests: '0',
    candidates,
    patterns,
    listMembers: false,
    isDefault: false,
  }
}

// Client-side guard before persisting generated routes: drop candidates whose
// provider is missing/disabled or no longer serves the model, then drop routes
// left without candidates or whose slug already exists.
export function filterGeneratedRoutes(routes: GeneratedRoute[], providers: { id: number; name: string; enabled: boolean; modelsList: string[] }[], existingSlugs: string[]): GeneratedRoute[] {
  const slugs = new Set(existingSlugs)
  const result: GeneratedRoute[] = []
  for (const route of routes) {
    if (slugs.has(route.slug)) continue
    // Generated candidates carry the provider name from generation time; a
    // rename would make routeInput resolve by a stale name, so rewrite it from
    // the current provider record while filtering.
    const candidates = route.candidates.flatMap((candidate) => {
      const provider = providers.find((item) => item.id === candidate.provider_id)
      if (!provider || !provider.enabled || !provider.modelsList.includes(candidate.model)) return []
      return [{ ...candidate, provider: provider.name }]
    })
    if (!candidates.length) continue
    slugs.add(route.slug)
    result.push({ ...route, candidates })
  }
  return result
}
