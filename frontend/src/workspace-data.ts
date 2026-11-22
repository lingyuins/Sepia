import {
  apiClient,
  mapPlugin,
  mapProvider,
  mapRequest,
  mapRoute,
  type ApiAnalytics,
  type ApiClientKey,
  type ApiGroup,
  type ApiModelGroup,
  type ApiPlugin,
  type ApiProvider,
  type ApiRequest,
  type ApiRoute,
  type ApiSettings,
} from './api.ts'
import type { ChannelGroup, Plugin, Provider, RequestRecord, RouteRule, Settings } from './workspace.tsx'

export const workspaceDomains = [
  'providers',
  'routes',
  'requests',
  'analytics',
  'keys',
  'groups',
  'modelGroups',
  'settings',
  'plugins',
] as const

export type WorkspaceDomain = typeof workspaceDomains[number]

export type WorkspaceData = {
  providers: Provider[]
  routes: RouteRule[]
  requests: RequestRecord[]
  analytics: ApiAnalytics
  clientKeys: ApiClientKey[]
  channelGroups: ChannelGroup[]
  modelGroups: ApiModelGroup[]
  plugins: Plugin[]
  version: string
  settings: Settings
}

export const emptyAnalytics: ApiAnalytics = {
  requests: 0,
  successful: 0,
  input_tokens: 0,
  output_tokens: 0,
  cost_micros: 0,
}

export function mergeServerSettings(current: Settings, server: ApiSettings): Settings {
  return {
    ...current,
    retries: String(server.max_attempts ?? current.retries),
    connectTimeout: String(server.connect_timeout_seconds ?? current.connectTimeout),
    firstByteTimeout: String(server.first_byte_timeout_seconds ?? current.firstByteTimeout),
    streamIdleTimeout: String(server.stream_idle_timeout_seconds ?? current.streamIdleTimeout),
    retryAfterCap: String(server.retry_after_cap_seconds ?? current.retryAfterCap),
    logBodies: server.log_bodies ?? current.logBodies,
    normalizePrefixes: server.model_normalize_router_prefixes ?? current.normalizePrefixes,
    normalizeSuffixes: server.model_normalize_functional_suffixes ?? current.normalizeSuffixes,
    normalizeMappings: server.model_normalize_explicit_mappings ?? current.normalizeMappings,
    aiPoolEnabled: server.ai_pool_enabled ?? current.aiPoolEnabled,
    workspace: server.workspace_name ?? current.workspace,
    deployment: server.gateway_deployment ?? current.deployment,
    protocol: server.gateway_protocol ?? current.protocol,
    host: server.gateway_host ?? current.host,
    port: server.gateway_port ?? current.port,
  }
}

function enrichProviders(providers: Provider[], analytics: ApiAnalytics): Provider[] {
  const stats = analytics.by_provider || []
  return providers.map((provider) => {
    const stat = stats.find((item) => item.provider_id === provider.id)
    return {
      ...provider,
      latency: Math.round(stat?.latency_ms || 0),
      requests: String(stat?.requests || 0),
      success: stat?.requests ? `${(stat.successful / stat.requests * 100).toFixed(2)}%` : '—',
    }
  })
}

export async function loadWorkspaceSnapshot(
  fetcher: typeof fetch,
  current: WorkspaceData | (() => WorkspaceData),
  domains: readonly WorkspaceDomain[],
  period: string,
  includeVersion: boolean,
  commit?: (next: WorkspaceData) => WorkspaceData | void,
): Promise<WorkspaceData> {
  const client = apiClient(fetcher)
  let snapshot = typeof current === 'function' ? current() : current
  const requested = new Set(domains)
  const needsRoutes = requested.has('routes') || requested.has('providers')
  const tasks: { domain: WorkspaceDomain | 'version'; promise: Promise<unknown> }[] = []
  if (requested.has('providers')) tasks.push({ domain: 'providers', promise: client<ApiProvider[]>('/api/v1/providers') })
  if (needsRoutes) tasks.push({ domain: 'routes', promise: client<ApiRoute[]>('/api/v1/routes') })
  if (requested.has('requests')) tasks.push({ domain: 'requests', promise: client<ApiRequest[]>('/api/v1/requests') })
  if (requested.has('analytics')) tasks.push({ domain: 'analytics', promise: client<ApiAnalytics>(`/api/v1/analytics?period=${encodeURIComponent(period)}`) })
  if (requested.has('keys')) tasks.push({ domain: 'keys', promise: client<ApiClientKey[]>('/api/v1/keys') })
  if (requested.has('groups')) tasks.push({ domain: 'groups', promise: client<ApiGroup[]>('/api/v1/groups') })
  if (requested.has('modelGroups')) tasks.push({ domain: 'modelGroups', promise: client<{ groups: ApiModelGroup[] }>('/api/v1/model-groups') })
  if (requested.has('settings')) tasks.push({ domain: 'settings', promise: client<ApiSettings>('/api/v1/settings') })
  if (requested.has('plugins')) tasks.push({ domain: 'plugins', promise: client<ApiPlugin[]>('/api/v1/plugins') })
  if (includeVersion) tasks.push({ domain: 'version', promise: client<{ version: string }>('/api/v1/version') })

  const entries = await Promise.all(tasks.map(async ({ domain, promise }) => [domain, await promise] as const))
  const raw = Object.fromEntries(entries) as Partial<{
    providers: ApiProvider[]
    routes: ApiRoute[]
    requests: ApiRequest[]
    analytics: ApiAnalytics
    keys: ApiClientKey[]
    groups: ApiGroup[]
    modelGroups: { groups: ApiModelGroup[] }
    settings: ApiSettings
    plugins: ApiPlugin[]
    version: { version: string }
  }>

  const next: WorkspaceData = { ...snapshot }
  if (raw.providers || raw.analytics) {
    const baseProviders = raw.providers ? raw.providers.map(mapProvider) : snapshot.providers
    next.providers = enrichProviders(baseProviders, raw.analytics ?? snapshot.analytics)
  }
  if (raw.routes) next.routes = raw.routes.map((route) => mapRoute(route, next.providers))
  if (raw.requests) next.requests = raw.requests.map(mapRequest)
  if (raw.analytics) next.analytics = raw.analytics
  if (raw.keys) next.clientKeys = raw.keys
  if (raw.groups) next.channelGroups = raw.groups
  if (raw.modelGroups) next.modelGroups = raw.modelGroups.groups
  if (raw.settings) next.settings = mergeServerSettings(snapshot.settings, raw.settings)
  if (raw.plugins) next.plugins = raw.plugins.map(mapPlugin)
  if (raw.version) next.version = raw.version.version
  if (typeof current === 'function') {
    snapshot = current()
    if (raw.providers && !raw.analytics) {
      next.providers = enrichProviders(next.providers, snapshot.analytics)
    }
    const changed = new Set<WorkspaceDomain>(requested)
    if (raw.routes) changed.add('routes')
    if (raw.analytics) changed.add('providers')
    for (const domain of workspaceDomains) {
      if (changed.has(domain)) continue
      const key = domain === 'keys' ? 'clientKeys' : domain === 'groups' ? 'channelGroups' : domain
      Object.assign(next, { [key]: snapshot[key] })
    }
    if (!includeVersion) next.version = snapshot.version
    return commit?.(next) ?? next
  }
  return next
}
