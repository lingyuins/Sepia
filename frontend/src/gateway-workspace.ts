import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError, api, mapProvider, mapRequest, mapRoute, mapPlugin, type ApiAnalytics, type ApiProvider, type ApiRequest, type ApiRoute, type ApiClientKey, type ApiGroup, type ApiSettings, type ApiPlugin } from './api'
import { defaultSettings } from './settings'
import { applyTheme } from './theme'
import { type ChannelGroup, type Plugin, type Provider, type RequestRecord, type RouteRule, type Settings } from './workspace'

export type ConnectionState = 'loading' | 'ready' | 'login' | 'error'
export type GatewayStatus = { session: boolean; initialized: boolean }
export type UserRole = 'admin' | 'editor' | 'viewer'
export type ConsoleSession = { username: string; role: UserRole; can_edit: boolean }
export function useGatewayWorkspace(): GatewayWorkspace {
  const [status, setStatus] = useState<ConnectionState>('loading')
  const [initialization, setInitialization] = useState<boolean | null>(null)
  const [error, setError] = useState('')
  const [session, setSession] = useState<ConsoleSession | null>(null)
  const [providers, setProviders] = useState<Provider[]>([])
  const [routes, setRoutes] = useState<RouteRule[]>([])
  const [requests, setRequests] = useState<RequestRecord[]>([])
  const [analytics, setAnalytics] = useState<ApiAnalytics>({ requests: 0, successful: 0, input_tokens: 0, output_tokens: 0, cost_micros: 0 })
  const [clientKeys, setClientKeys] = useState<ApiClientKey[]>([])
  const [settings, setSettings] = useState<Settings>(() => { try { const stored = JSON.parse(localStorage.getItem('sepia.workspace.v1') || '{}'); const merged = { ...defaultSettings, ...stored.settings }; return stored.settings?.port === '3425' ? { ...merged, port: defaultSettings.port } : merged } catch { return defaultSettings } })
  const applyServerSettings = useCallback((server: ApiSettings) => { setSettings((current) => ({ ...current, retries: String(server.max_attempts ?? current.retries), logBodies: server.log_bodies ?? current.logBodies })) }, [])
  const [channelGroups, setChannelGroups] = useState<ChannelGroup[]>([])
  const [plugins, setPlugins] = useState<Plugin[]>([])
  const [version, setVersion] = useState('')
  const [toast, setToast] = useState('')
  const sequence = useRef(0)
  const periodRef = useRef('30 days')
  const refresh = useCallback(async (period = periodRef.current) => {
    periodRef.current = period
    const current = ++sequence.current
    try {
      const [providerData, routeData, requestData, analyticsData, keyData, groupData, serverSettings, pluginData, versionData] = await Promise.all([
        api<ApiProvider[]>('/api/v1/providers'), api<ApiRoute[]>('/api/v1/routes'), api<ApiRequest[]>('/api/v1/requests'), api<ApiAnalytics>(`/api/v1/analytics?period=${encodeURIComponent(period)}`), api<ApiClientKey[]>('/api/v1/keys'), api<ApiGroup[]>('/api/v1/groups'), api<ApiSettings>('/api/v1/settings'), api<ApiPlugin[]>('/api/v1/plugins'), api<{ version: string }>('/api/v1/version'),
      ])
      if (current !== sequence.current) return
      const mappedProviders = providerData.map(mapProvider)
      const providerStats = analyticsData.by_provider || []
      setProviders(mappedProviders.map((provider) => { const stat = providerStats.find((item) => item.provider_id === provider.id); return { ...provider, latency: Math.round(stat?.latency_ms || 0), requests: String(stat?.requests || 0), success: stat?.requests ? `${(stat.successful / stat.requests * 100).toFixed(2)}%` : '—' } }))
      setRoutes(routeData.map((route) => mapRoute(route, mappedProviders)))
      setRequests(requestData.map(mapRequest)); setAnalytics(analyticsData); setClientKeys(keyData); setChannelGroups(groupData); setPlugins(pluginData.map(mapPlugin)); setVersion(versionData.version); applyServerSettings(serverSettings); setStatus('ready'); setError('')
    } catch (cause) {
      if (current !== sequence.current) return
      setError(cause instanceof Error ? cause.message : 'Gateway unavailable')
      setStatus('error')
      throw cause
    }
  }, [applyServerSettings])
  const checkGateway = useCallback(async (): Promise<GatewayStatus> => {
    const current = ++sequence.current
    setError('')
    try {
      const [sessionInfo, gatewayStatus] = await Promise.all([
        api<ConsoleSession>('/api/v1/auth/session').then((info) => info).catch((cause) => {
          if (cause instanceof ApiError && cause.status === 401) return null
          throw cause
        }),
        api<{ initialized: boolean }>('/api/v1/auth/status'),
      ])
      const result = { session: sessionInfo !== null, initialized: gatewayStatus.initialized }
      if (current !== sequence.current) return result
      setSession(sessionInfo)
      setInitialization(result.initialized)
      if (result.session) await refresh()
      else setStatus('login')
      return result
    } catch (cause) {
      if (current === sequence.current) {
        setError(cause instanceof Error ? cause.message : 'Gateway unavailable')
        setStatus('error')
      }
      throw cause
    }
  }, [refresh])
  const run = useCallback(async <T,>(operation: () => Promise<T>) => { const result = await operation(); await refresh(periodRef.current); return result }, [refresh])
  const login = useCallback(async (username: string, password: string) => {
    ++sequence.current
    const info = await api<ConsoleSession>('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ username, password }) })
    setSession({ username: info.username, role: info.role, can_edit: info.role !== 'viewer' })
    setInitialization(true)
    await refresh()
  }, [refresh])
  const loginWithPasskey = useCallback(async (credential: Record<string, unknown>, challenge: string) => {
    ++sequence.current
    const info = await api<ConsoleSession>('/api/v1/auth/passkey/login/finish', { method: 'POST', body: JSON.stringify({ challenge, credential }) })
    setSession({ username: info.username, role: info.role, can_edit: info.role !== 'viewer' })
    setInitialization(true)
    await refresh()
  }, [refresh])
  const logout = useCallback(async () => { await api('/api/v1/auth/logout', { method: 'POST' }); ++sequence.current; setSession(null); setStatus('login') }, [])
  useEffect(() => { void checkGateway().catch(() => {}) }, [checkGateway])
  useEffect(() => { try { const previous = JSON.parse(localStorage.getItem('sepia.workspace.v1') || '{}'); localStorage.setItem('sepia.workspace.v1', JSON.stringify({ settings: { ...defaultSettings, ...previous.settings, ...settings } })) } catch { /* Browser storage may be disabled. */ } }, [settings])
  useEffect(() => { applyTheme(document.documentElement, settings, window.matchMedia('(prefers-color-scheme: light)').matches) }, [settings])
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(''), 3500); return () => clearTimeout(timer) }, [toast])
  return { status, initialization, error, session, providers, routes, requests, analytics, clientKeys, settings, setSettings, channelGroups, setChannelGroups, plugins, version, toast, notify: setToast, clearToast: () => setToast(''), refresh, checkGateway, run, login, loginWithPasskey, logout }
}
export interface GatewayWorkspace {
  status: ConnectionState; initialization: boolean | null; error: string; session: ConsoleSession | null; providers: Provider[]; routes: RouteRule[]; requests: RequestRecord[]; analytics: ApiAnalytics; clientKeys: ApiClientKey[]
  settings: Settings; setSettings: React.Dispatch<React.SetStateAction<Settings>>; channelGroups: ChannelGroup[]; setChannelGroups: React.Dispatch<React.SetStateAction<ChannelGroup[]>>
  plugins: Plugin[]; version: string; toast: string; notify: (message: string) => void; clearToast: () => void
  refresh: (period?: string) => Promise<void>; checkGateway: () => Promise<GatewayStatus>; run: <T>(operation: () => Promise<T>) => Promise<T>; login: (username: string, password: string) => Promise<void>; loginWithPasskey: (credential: Record<string, unknown>, challenge: string) => Promise<void>; logout: () => Promise<void>
}
