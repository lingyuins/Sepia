import type { RouteCandidate, RouteRule } from './workspace'

export type AiPoolService = {
  id: number
  name: string
  endpoint: string
  model: string
  models: string[]
  timeoutSeconds: number
  concurrency: number
  enabled: boolean
}

export type AiPoolServiceDraft = {
  name: string
  endpoint: string
  secret: string
  model: string
  models: string[]
  timeoutSeconds: string
  concurrency: string
  enabled: boolean
}

export const emptyAiServiceDraft: AiPoolServiceDraft = { name: '', endpoint: '', secret: '', model: '', models: [], timeoutSeconds: '60', concurrency: '2', enabled: true }

export type AiServiceDraftError = { key: string; params?: Record<string, string> }

// Mirrors backend validate(): name/endpoint/model required, timeout 1..600s,
// concurrency 1..16. A blank secret is allowed when editing (keeps stored key).
export function validateAiServiceDraft(draft: AiPoolServiceDraft, editing: boolean): AiServiceDraftError | null {
  if (!draft.name.trim()) return { key: 'aiServiceNameRequired' }
  const endpoint = draft.endpoint.trim()
  let parsed: URL
  try { parsed = new URL(endpoint) } catch { return { key: 'aiServiceEndpointInvalid' } }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return { key: 'aiServiceEndpointInvalid' }
  if (!editing && !draft.secret.trim()) return { key: 'aiServiceKeyRequired' }
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
    endpoint: draft.endpoint.trim(),
    secret: draft.secret.trim(),
    model: draft.model.trim(),
    models: draft.models,
    timeout_seconds: Number(draft.timeoutSeconds),
    concurrency: Number(draft.concurrency),
    enabled: draft.enabled,
  }
}

export type ApiAiPoolService = { id: number; name: string; endpoint: string; model: string; models: string[]; timeout_seconds: number; concurrency: number; enabled: boolean; created_at: string; updated_at: string }

export function mapAiPoolService(service: ApiAiPoolService): AiPoolService {
  return { id: service.id, name: service.name, endpoint: service.endpoint, model: service.model, models: service.models, timeoutSeconds: service.timeout_seconds, concurrency: service.concurrency, enabled: service.enabled }
}

export function serviceDraftFrom(service: AiPoolService): AiPoolServiceDraft {
  return { name: service.name, endpoint: service.endpoint, secret: '', model: service.model, models: service.models, timeoutSeconds: String(service.timeoutSeconds), concurrency: String(service.concurrency), enabled: service.enabled }
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
    primary: candidates[0]?.provider || '',
    primaryModel: candidates[0]?.model || '',
    fallback: candidates[1]?.provider || '',
    fallbackModel: candidates[1]?.model || '',
    strategy: 'Failover',
    active: true,
    requests: '0',
    candidates,
    patterns,
    listMembers: true,
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
    const candidates = route.candidates.filter((candidate) => {
      const provider = providers.find((item) => item.id === candidate.provider_id)
      return !!provider && provider.enabled && provider.modelsList.includes(candidate.model)
    })
    if (!candidates.length) continue
    slugs.add(route.slug)
    result.push({ ...route, candidates })
  }
  return result
}
