import type { KeyPolicy } from './key-policy'
import type { AttemptRecord, Plugin, Provider, RequestRecord, RouteRule } from './workspace'
import { clampWeight, normalizeRouteStrategy } from './route-strategy.ts'
type ProviderProtocol = 'openai' | 'anthropic' | 'gemini' | 'openai-responses'

export type ApiProvider = { id: number; name: string; protocol: ProviderProtocol; endpoint: string; models: string[]; key_count: number; enabled: boolean; group_id: string; keys?: { id: number; models: string[] | null }[] }
export type ApiRoute = { id: number; slug: string; name: string; description: string; patterns: string[]; is_default: boolean; list_members: boolean; enabled: boolean; candidates: [number, string][]; strategy?: string; weights?: number[] }
export type ApiRequest = { id: string; protocol: string; model: string; route?: string; status: string; input_tokens: number; output_tokens: number; cost_micros: number; latency_ms: number; created_at: string; provider?: string; key?: string; client_ip?: string | null; first_byte_ms?: number | null; cached_tokens?: number; reasoning_tokens?: number; has_body?: boolean }
export type ApiAttempt = { id: number; request_id: string; provider_id: number | null; provider_key_id: number | null; model: string; status: string; error: string | null; input_tokens: number; output_tokens: number; latency_ms: number; created_at: string }
export type ApiClientKey = { id: number; name: string; models: string[]; token_limit: number; max_amount_micros: number; rpm: number; tpm: number; expires_at: string | null }
export type ApiGroup = { id: string; name: string; description: string }
export type ApiPlugin = { id: string; name: string; version: string; enabled: boolean; status: string; description: string; events: string[]; permissions: string[]; priority: number; has_ui: boolean; ui_title: string; config_schema: Record<string, unknown>; config: Record<string, unknown>; sha256: string; installed_at: string; updated_at: string }
export type ApiSettings = Partial<{ connect_timeout_seconds: number; first_byte_timeout_seconds: number; stream_idle_timeout_seconds: number; max_attempts: number; retry_after_cap_seconds: number; log_bodies: boolean; body_retention_days: number; model_normalize_router_prefixes: string[]; model_normalize_functional_suffixes: string[]; model_normalize_explicit_mappings: { variant: string; canonical: string }[]; ai_pool_enabled: boolean }>
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
    const isForm = typeof FormData !== 'undefined' && options.body instanceof FormData
    const response = await fetcher(url, { credentials: 'same-origin', ...options, headers: { ...(options.body && !isForm ? { 'content-type': 'application/json' } : {}), ...options.headers } })
    const body = await response.json().catch(() => ({}))
    if (!response.ok) throw new ApiError(body.error?.message || `Request failed (${response.status})`, response.status)
    return body as T
  }
}
export const api = apiClient()
const color = ['#dcf1d2', '#e8cdb7', '#c5d9ef', '#c8c5ee']
export function mapProvider(provider: ApiProvider): Provider {
  return { id: provider.id, name: provider.name, short: provider.name.charAt(0).toUpperCase(), type: provider.protocol, models: provider.models.length, latency: 0, success: '—', requests: '0', color: color[provider.id % color.length], enabled: provider.enabled, endpoint: provider.endpoint, group: provider.group_id, modelsList: provider.models, keyCount: provider.key_count, keyScopes: provider.keys }
}
export function mapRoute(route: ApiRoute, providers: Provider[]): RouteRule {
  const candidates = route.candidates.map(([provider, model], index) => ({ id: `${route.id}-${index}`, provider: providers.find((item) => item.id === provider)?.name || String(provider), model, enabled: true, weight: clampWeight(route.weights?.[index] ?? 1) }))
  const [primary, fallback] = candidates
  return { id: route.id, name: route.name || route.slug, slug: route.slug, description: route.description || '', match: route.patterns[0] || '*', primary: primary?.provider || '', primaryModel: primary?.model || '', fallback: fallback?.provider || '', fallbackModel: fallback?.model || '', strategy: normalizeRouteStrategy(route.strategy), active: route.enabled, requests: '0', candidates, patterns: route.patterns, listMembers: route.list_members, isDefault: route.is_default }
}
export function mapRequest(request: ApiRequest): RequestRecord {
  const total = request.input_tokens + request.output_tokens
  const time = new Date(request.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
  return { id: request.id, createdAt: request.created_at, time, model: request.model || 'unknown', provider: request.provider || '—', tokens: total.toLocaleString('en-US'), inputTokens: request.input_tokens, outputTokens: request.output_tokens, latency: `${request.latency_ms} ms`, latencyMs: request.latency_ms, status: request.status === 'success' ? '200 OK' : request.status === 'failed' ? 'Failed' : request.status, key: request.key || '—', cost: `$${(request.cost_micros / 1000000).toFixed(6)}`, route: request.route || '—', clientIp: request.client_ip ?? null, firstByteMs: request.first_byte_ms ?? null, cachedTokens: request.cached_tokens ?? 0, reasoningTokens: request.reasoning_tokens ?? 0, protocol: request.protocol, hasBody: request.has_body === true }
}
export function mapAttempt(attempt: ApiAttempt): AttemptRecord {
  return { id: attempt.id, requestId: attempt.request_id, providerId: attempt.provider_id, providerKeyId: attempt.provider_key_id, model: attempt.model, status: attempt.status, error: attempt.error, inputTokens: attempt.input_tokens, outputTokens: attempt.output_tokens, latencyMs: attempt.latency_ms, createdAt: attempt.created_at }
}
export function mapPlugin(plugin: ApiPlugin): Plugin {
  return { id: plugin.id, name: plugin.name, description: plugin.description, version: plugin.version, enabled: plugin.enabled, status: plugin.status, events: plugin.events, permissions: plugin.permissions, priority: plugin.priority, hasUi: plugin.has_ui, uiTitle: plugin.ui_title, configSchema: plugin.config_schema || {}, config: plugin.config || {}, sha256: plugin.sha256, installedAt: plugin.installed_at, updatedAt: plugin.updated_at }
}
export type ProviderKeyDraft = { value: string; models: string[] | null }
export function providerInput(provider: Provider, keyDrafts: ProviderKeyDraft[] = []) {
  const entered = keyDrafts.filter((key) => key.value.trim())
  return { name: provider.name, protocol: provider.type, endpoint: provider.endpoint, models: provider.modelsList, secret: entered[0]?.value.trim() || '', secrets: entered.map((key) => key.value.trim()), key_models: entered.filter((key) => key.models !== null).map((key) => ({ secret: key.value.trim(), models: key.models })), group_id: provider.group, enabled: provider.enabled }
}
export function keyInput(policy: KeyPolicy) {
  const expiresAt = policy.expiresAt ? policy.expiresAt.slice(0, 16) : null
  return { name: policy.name, models: policy.models, token_limit: policy.tokenLimit || 0, max_amount_micros: Math.round((policy.maxAmount || 0) * 1000000), rpm: policy.rpm, tpm: policy.tpm, expires_at: expiresAt }
}
export function policyInput(policy: KeyPolicy, providers: Provider[]) {
  return { ip_whitelist: policy.ipWhitelist, model_quotas: policy.modelQuotas, allowed_groups: policy.allowedGroups, excluded_providers: policy.excludedProviders.map((name) => { const provider = providers.find((item) => item.name === name || String(item.id) === name); if (!provider) throw new Error(`Unknown provider: ${name}`); return provider.id }) }
}
export function routeInput(route: RouteRule, providers: Provider[]) {
  const enabled = (route.candidates || []).filter((candidate) => candidate.enabled)
  const candidates = enabled.map((candidate) => { const provider = providers.find((item) => item.name === candidate.provider); if (!provider) throw new Error(`Unknown provider: ${candidate.provider}`); return [provider.id, candidate.model] as [number, string] })
  return { slug: route.slug, name: route.name, description: route.description, patterns: route.patterns || [route.match], is_default: route.isDefault === true, list_members: route.listMembers !== false, enabled: route.active, candidates, strategy: normalizeRouteStrategy(route.strategy), weights: enabled.map((candidate) => clampWeight(candidate.weight ?? 1)) }
}
