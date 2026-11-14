import type { KeyPolicy } from './key-policy'
import type { Provider, RequestRecord, RouteRule } from './workspace'
type ProviderProtocol = 'openai' | 'anthropic' | 'gemini'

export type ApiProvider = { id: number; name: string; protocol: ProviderProtocol; endpoint: string; models: string[]; key_count: number; enabled: boolean; group_id: string }
export type ApiRoute = { id: number; slug: string; name: string; description: string; patterns: string[]; is_default: boolean; list_members: boolean; enabled: boolean; candidates: [number, string][] }
export type ApiRequest = { id: string; protocol: string; model: string; route?: string; status: string; input_tokens: number; output_tokens: number; cost_micros: number; latency_ms: number; created_at: string; provider?: string; key?: string }
export type ApiClientKey = { id: number; name: string; models: string[]; token_limit: number; max_amount_micros: number; rpm: number; tpm: number; expires_at: string | null }
export type ApiGroup = { id: string; name: string; description: string }
export type ApiSettings = Partial<{ connect_timeout_seconds: number; first_byte_timeout_seconds: number; stream_idle_timeout_seconds: number; max_attempts: number; retry_after_cap_seconds: number; log_bodies: boolean; body_retention_days: number }>
export type ApiAnalytics = { requests: number; successful: number; input_tokens: number; output_tokens: number; cost_micros: number; latency_ms?: number; series?: { time: string; requests: number; successful: number; cost_micros: number; latency_ms: number }[]; by_model?: { model: string; requests: number }[]; by_provider?: { provider_id: number; requests: number; successful: number; latency_ms: number }[] }

export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export function apiClient(fetcher: typeof fetch = fetch) {
  return async <T>(url: string, options: RequestInit = {}): Promise<T> => {
    const response = await fetcher(url, { credentials: 'same-origin', ...options, headers: { ...(options.body ? { 'content-type': 'application/json' } : {}), ...options.headers } })
    const body = await response.json().catch(() => ({}))
    if (!response.ok) throw new ApiError(body.error?.message || `Request failed (${response.status})`, response.status)
    return body as T
  }
}
export const api = apiClient()
const color = ['#dcf1d2', '#e8cdb7', '#c5d9ef', '#c8c5ee']
export function mapProvider(provider: ApiProvider): Provider {
  return { id: provider.id, name: provider.name, short: provider.name.charAt(0).toUpperCase(), type: provider.protocol, models: provider.models.length, latency: 0, success: '—', requests: '0', color: color[provider.id % color.length], enabled: provider.enabled, endpoint: provider.endpoint, group: provider.group_id, modelsList: provider.models, keyCount: provider.key_count }
}
export function mapRoute(route: ApiRoute, providers: Provider[]): RouteRule {
  const candidates = route.candidates.map(([provider, model], index) => ({ id: `${route.id}-${index}`, provider: providers.find((item) => item.id === provider)?.name || String(provider), model, enabled: true, weight: 1 }))
  const [primary, fallback] = candidates
  return { id: route.id, name: route.name || route.slug, slug: route.slug, description: route.description || '', match: route.patterns[0] || '*', primary: primary?.provider || '', primaryModel: primary?.model || '', fallback: fallback?.provider || '', fallbackModel: fallback?.model || '', strategy: 'Failover', active: route.enabled, requests: '0', candidates, patterns: route.patterns, listMembers: route.list_members, isDefault: route.is_default }
}
export function mapRequest(request: ApiRequest): RequestRecord {
  const total = request.input_tokens + request.output_tokens
  const time = new Date(request.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
  return { id: request.id, createdAt: request.created_at, time, model: request.model || 'unknown', provider: request.provider || '—', tokens: total.toLocaleString('en-US'), inputTokens: request.input_tokens, outputTokens: request.output_tokens, latency: `${request.latency_ms} ms`, latencyMs: request.latency_ms, status: request.status === 'success' ? '200 OK' : request.status === 'failed' ? 'Failed' : request.status, key: request.key || '—', cost: `$${(request.cost_micros / 1000000).toFixed(6)}`, route: request.route || '—' }
}
export function providerInput(provider: Provider & { keyDrafts?: { value: string }[] }, secrets?: string[]) {
  const entered = secrets || provider.keyDrafts?.map((key) => key.value).filter(Boolean) || []
  return { name: provider.name, protocol: provider.type, endpoint: provider.endpoint, models: provider.modelsList, secret: entered[0] || '', ...(secrets ? { secrets } : {}), group_id: provider.group, enabled: provider.enabled }
}
export function keyInput(policy: KeyPolicy) {
  const expiresAt = policy.expiresAt ? policy.expiresAt.slice(0, 16) : null
  return { name: policy.name, models: policy.models, token_limit: policy.tokenLimit || 0, max_amount_micros: Math.round((policy.maxAmount || 0) * 1000000), rpm: policy.rpm, tpm: policy.tpm, expires_at: expiresAt }
}
export function policyInput(policy: KeyPolicy, providers: Provider[]) {
  return { ip_whitelist: policy.ipWhitelist, model_quotas: policy.modelQuotas, allowed_groups: policy.allowedGroups, excluded_providers: policy.excludedProviders.map((name) => { const provider = providers.find((item) => item.name === name || String(item.id) === name); if (!provider) throw new Error(`Unknown provider: ${name}`); return provider.id }) }
}
export function routeInput(route: RouteRule, providers: Provider[]) {
  const candidates = (route.candidates || []).filter((candidate) => candidate.enabled).map((candidate) => { const provider = providers.find((item) => item.name === candidate.provider); if (!provider) throw new Error(`Unknown provider: ${candidate.provider}`); return [provider.id, candidate.model] as [number, string] })
  return { slug: route.slug, name: route.name, description: route.description, patterns: route.patterns || [route.match], is_default: route.isDefault === true, list_members: route.listMembers !== false, enabled: route.active, candidates }
}
