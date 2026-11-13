import { useCallback, useEffect, useRef, useState } from 'react'
import { api, mapProvider, mapRequest, mapRoute, type ApiAnalytics, type ApiProvider, type ApiRequest, type ApiRoute, type ApiClientKey, type ApiGroup, type ApiSettings } from './api'
import { defaultSettings, type ChannelGroup, type Notice, type Plugin, type Provider, type RequestRecord, type RouteRule, type Settings } from './workspace'

export type ConnectionState = 'loading' | 'ready' | 'login' | 'error'
export function useGatewayWorkspace(): GatewayWorkspace {
  const [status, setStatus] = useState<ConnectionState>('loading')
  const [error, setError] = useState('')
  const [providers, setProviders] = useState<Provider[]>([])
  const [routes, setRoutes] = useState<RouteRule[]>([])
  const [requests, setRequests] = useState<RequestRecord[]>([])
  const [analytics, setAnalytics] = useState<ApiAnalytics>({ requests: 0, successful: 0, input_tokens: 0, output_tokens: 0, cost_micros: 0 })
  const [clientKeys, setClientKeys] = useState<ApiClientKey[]>([])
  const [settings, setSettings] = useState<Settings>(() => { try { const stored = JSON.parse(localStorage.getItem('sepia.workspace.v1') || '{}'); return { ...defaultSettings, ...stored.settings } } catch { return defaultSettings } })
  const applyServerSettings = useCallback((server: ApiSettings) => { setSettings((current) => ({ ...current, retries: String(server.max_attempts ?? current.retries) })) }, [])
  const [channelGroups, setChannelGroups] = useState<ChannelGroup[]>([])
  const [plugins] = useState<Plugin[]>([])
  const [notices, setNotices] = useState<Notice[]>([])
  const [toast, setToast] = useState('')
  const sequence = useRef(0)
  const periodRef = useRef('30 days')
  const refresh = useCallback(async (period = periodRef.current) => {
    periodRef.current = period
    const current = ++sequence.current
    try {
      const [providerData, routeData, requestData, analyticsData, keyData, groupData, serverSettings] = await Promise.all([
        api<ApiProvider[]>('/api/v1/providers'), api<ApiRoute[]>('/api/v1/routes'), api<ApiRequest[]>('/api/v1/requests'), api<ApiAnalytics>(`/api/v1/analytics?period=${encodeURIComponent(period)}`), api<ApiClientKey[]>('/api/v1/keys'), api<ApiGroup[]>('/api/v1/groups'), api<ApiSettings>('/api/v1/settings'),
      ])
      if (current !== sequence.current) return
      const mappedProviders = providerData.map(mapProvider)
      const providerStats = analyticsData.by_provider || []
      setProviders(mappedProviders.map((provider) => { const stat = providerStats.find((item) => item.provider_id === provider.id); return { ...provider, latency: Math.round(stat?.latency_ms || 0), requests: String(stat?.requests || 0), success: stat?.requests ? `${(stat.successful / stat.requests * 100).toFixed(2)}%` : '—' } }))
      setRoutes(routeData.map((route) => mapRoute(route, mappedProviders)))
      setRequests(requestData.map(mapRequest)); setAnalytics(analyticsData); setClientKeys(keyData); setChannelGroups(groupData); applyServerSettings(serverSettings); setStatus('ready'); setError('')
    } catch (cause) {
      if (current !== sequence.current) return
      setError(cause instanceof Error ? cause.message : 'Gateway unavailable')
      setStatus('error')
      throw cause
    }
  }, [applyServerSettings])
  const run = useCallback(async <T,>(operation: () => Promise<T>) => { const result = await operation(); await refresh(periodRef.current); return result }, [refresh])
  const login = useCallback(async (username: string, password: string) => { await api('/api/v1/auth/login', { method: 'POST', body: JSON.stringify({ username, password }) }); await refresh() }, [refresh])
  const logout = useCallback(async () => { await api('/api/v1/auth/logout', { method: 'POST' }); setStatus('login') }, [])
  useEffect(() => {
    api<{ username: string }>('/api/v1/auth/session').then(() => refresh()).catch((cause) => { setError(cause instanceof Error ? cause.message : 'Gateway unavailable'); setStatus('login') })
  }, [refresh])
  useEffect(() => { try { const previous = JSON.parse(localStorage.getItem('sepia.workspace.v1') || '{}'); localStorage.setItem('sepia.workspace.v1', JSON.stringify({ settings: { ...defaultSettings, ...previous.settings, ...settings } })) } catch { /* Browser storage may be disabled. */ } }, [settings])
  useEffect(() => { const colors: Record<string, string> = { Sage: '#c4eaaa', Sky: '#a5cfe5', Lavender: '#c3b3e5', Sand: '#dfc49f' }; const root = document.documentElement; root.dataset.theme = settings.theme === 'Light' || (settings.theme === 'System' && window.matchMedia('(prefers-color-scheme: light)').matches) ? 'light' : 'dark'; root.style.setProperty('--accent', colors[settings.accent] || colors.Sage); root.classList.toggle('reduce-motion', settings.reducedMotion); root.classList.toggle('compact-tables', settings.compact); root.lang = settings.language === '简体中文' ? 'zh-CN' : settings.language === '日本語' ? 'ja' : 'en' }, [settings])
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(''), 3500); return () => clearTimeout(timer) }, [toast])
  return { status, error, providers, routes, requests, analytics, clientKeys, settings, setSettings, channelGroups, setChannelGroups, plugins, notices, setNotices, toast, notify: setToast, clearToast: () => setToast(''), refresh, run, login, logout }
}
export interface GatewayWorkspace {
  status: ConnectionState; error: string; providers: Provider[]; routes: RouteRule[]; requests: RequestRecord[]; analytics: ApiAnalytics; clientKeys: ApiClientKey[]
  settings: Settings; setSettings: React.Dispatch<React.SetStateAction<Settings>>; channelGroups: ChannelGroup[]; setChannelGroups: React.Dispatch<React.SetStateAction<ChannelGroup[]>>
  plugins: Plugin[]; notices: Notice[]; setNotices: React.Dispatch<React.SetStateAction<Notice[]>>; toast: string; notify: (message: string) => void; clearToast: () => void
  refresh: (period?: string) => Promise<void>; run: <T>(operation: () => Promise<T>) => Promise<T>; login: (username: string, password: string) => Promise<void>; logout: () => Promise<void>
}
