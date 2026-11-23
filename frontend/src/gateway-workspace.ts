import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ApiError, api } from './api'
import { LiveRefreshCoalescer, parseLiveEvent, type LiveRefreshDomains } from './live-refresh'
import { defaultSettings } from './settings'
import { applyTheme } from './theme'
import type { Settings } from './workspace'
import {
  emptyAnalytics,
  loadWorkspaceSnapshot,
  workspaceDomains,
  type WorkspaceData,
  type WorkspaceDomain,
} from './workspace-data'

export type ConnectionState = 'loading' | 'ready' | 'login' | 'error'
export type GatewayStatus = { session: boolean; initialized: boolean }
export type UserRole = 'admin' | 'editor' | 'viewer'
export type ConsoleSession = { username: string; role: UserRole; can_edit: boolean }

export function readSettings(): Settings {
  try {
    const stored = JSON.parse(localStorage.getItem('sepia.workspace.v1') || '{}')
    const merged = { ...defaultSettings, ...stored.settings }
    return stored.settings?.port === '3425' ? { ...merged, port: defaultSettings.port } : merged
  } catch {
    return defaultSettings
  }
}

export function useGatewayWorkspace(): GatewayWorkspace {
  const [status, setStatus] = useState<ConnectionState>('loading')
  const [initialization, setInitialization] = useState<boolean | null>(null)
  const [error, setError] = useState('')
  const [session, setSession] = useState<ConsoleSession | null>(null)
  const [data, setData] = useState<WorkspaceData>(() => ({
    providers: [],
    routes: [],
    requests: [],
    analytics: emptyAnalytics,
    clientKeys: [],
    channelGroups: [],
    modelGroups: [],
    plugins: [],
    version: '',
    settings: readSettings(),
  }))
  const dataRef = useRef(data)
  const sequence = useRef(0)
  const periodRef = useRef('30 days')
  const refresh = useCallback(async (
    period = periodRef.current,
    domains: readonly WorkspaceDomain[] = workspaceDomains,
    options: { background?: boolean } = {},
  ) => {
    const background = options.background === true
    periodRef.current = period
    const current = ++sequence.current
    const includeVersion = domains.length === workspaceDomains.length && !dataRef.current.version
    try {
      await loadWorkspaceSnapshot(fetch, () => dataRef.current, domains, period, includeVersion, (next) => {
        if (current !== sequence.current) return
        dataRef.current = next
        setData(next)
        return next
      })
      if (current !== sequence.current) return
      setStatus('ready')
      setError('')
    } catch (cause) {
      if (current !== sequence.current) return
      // Only an expired session changes the gate. A background (event-driven)
      // refresh otherwise keeps the last good data and never drops a working
      // console into the offline screen.
      if (cause instanceof ApiError && cause.status === 401) {
        setSession(null)
        setStatus('login')
        throw cause
      }
      if (background) throw cause
      setError(cause instanceof Error ? cause.message : 'Gateway unavailable')
      setStatus('error')
      throw cause
    }
  }, [])
  // Live event pushes and mutation-driven `run` refreshes share one debounce
  // pipeline, so a burst of pushes (or a push racing its own mutation) costs a
  // single workspace read.
  const live = useMemo(() => new LiveRefreshCoalescer(
    (domains, background) => refresh(
      periodRef.current,
      domains === 'all' ? workspaceDomains : domains,
      { background },
    ),
  ), [refresh])
  const checkGateway = useCallback(async (): Promise<GatewayStatus> => {
    const current = ++sequence.current
    setError('')
    try {
      const [sessionInfo, gatewayStatus] = await Promise.all([
        api<ConsoleSession>('/api/v1/auth/session').catch((cause) => {
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
  const run = useCallback(async <Value,>(
    operation: () => Promise<Value>,
    domains?: readonly WorkspaceDomain[],
  ) => {
    const result = await operation()
    await live.schedule(domains ?? 'all', false)
    return result
  }, [live])
  const login = useCallback(async (username: string, password: string) => {
    ++sequence.current
    const info = await api<ConsoleSession>('/api/v1/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    })
    setSession({ username: info.username, role: info.role, can_edit: info.role !== 'viewer' })
    setInitialization(true)
    await refresh()
  }, [refresh])
  const loginWithPasskey = useCallback(async (credential: Record<string, unknown>, challenge: string) => {
    ++sequence.current
    const info = await api<ConsoleSession>('/api/v1/auth/passkey/login/finish', {
      method: 'POST',
      body: JSON.stringify({ challenge, credential }),
    })
    setSession({ username: info.username, role: info.role, can_edit: info.role !== 'viewer' })
    setInitialization(true)
    await refresh()
  }, [refresh])
  const logout = useCallback(async () => {
    await api('/api/v1/auth/logout', { method: 'POST' })
    ++sequence.current
    setSession(null)
    setStatus('login')
  }, [])

  // Subscribe to gateway change hints once a session exists; EventSource
  // reconnects on its own, and a re-open means events may have been missed, so
  // it asks for one full refresh.
  const liveSession = session !== null && status !== 'login'
  useEffect(() => {
    if (!liveSession) return
    const dirty = new Set<WorkspaceDomain>()
    let dirtyAll = false
    let connected = false
    const source = new EventSource('/api/v1/events')
    const request = (domains: LiveRefreshDomains) => {
      if (document.hidden) {
        if (domains === 'all') dirtyAll = true
        else for (const domain of domains) dirty.add(domain)
        return
      }
      void live.schedule(domains).catch(() => {
        // Background failures keep the last good data; the next event or a
        // reconnect retries. A 401 has already moved the console to login.
      })
    }
    source.onopen = () => {
      if (connected) request('all')
      connected = true
    }
    source.onmessage = (message) => {
      const domains = parseLiveEvent(message.data)
      if (domains !== null) request(domains)
    }
    const onVisibility = () => {
      if (document.hidden) return
      if (!dirtyAll && dirty.size === 0) return
      const domains: LiveRefreshDomains = dirtyAll ? 'all' : [...dirty]
      dirty.clear()
      dirtyAll = false
      request(domains)
    }
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      document.removeEventListener('visibilitychange', onVisibility)
      source.close()
      live.cancel()
    }
  }, [live, liveSession])

  const setSettings = useCallback((update: React.SetStateAction<Settings>) => {
    setData((current) => {
      const settings = typeof update === 'function' ? update(current.settings) : update
      if (settings === current.settings) return current
      const next = { ...current, settings }
      dataRef.current = next
      return next
    })
  }, [])

  const setChannelGroups = useCallback((update: React.SetStateAction<WorkspaceData['channelGroups']>) => {
    setData((current) => {
      const channelGroups = typeof update === 'function' ? update(current.channelGroups) : update
      if (channelGroups === current.channelGroups) return current
      const next = { ...current, channelGroups }
      dataRef.current = next
      return next
    })
  }, [])

  useEffect(() => {
    void checkGateway().catch(() => {})
  }, [checkGateway])

  useEffect(() => {
    try {
      localStorage.setItem('sepia.workspace.v1', JSON.stringify({ settings: data.settings }))
    } catch {}
  }, [data.settings])

  useEffect(() => {
    applyTheme(document.documentElement, data.settings, window.matchMedia('(prefers-color-scheme: light)').matches)
  }, [data.settings])

  return {
    status,
    initialization,
    error,
    session,
    ...data,
    setSettings,
    setChannelGroups,
    refresh,
    checkGateway,
    run,
    login,
    loginWithPasskey,
    logout,
  }
}

export interface GatewayWorkspace extends WorkspaceData {
  status: ConnectionState
  initialization: boolean | null
  error: string
  session: ConsoleSession | null
  setSettings: React.Dispatch<React.SetStateAction<Settings>>
  setChannelGroups: React.Dispatch<React.SetStateAction<WorkspaceData['channelGroups']>>
  refresh: (period?: string, domains?: readonly WorkspaceDomain[], options?: { background?: boolean }) => Promise<void>
  checkGateway: () => Promise<GatewayStatus>
  run: <Value>(operation: () => Promise<Value>, domains?: readonly WorkspaceDomain[]) => Promise<Value>
  login: (username: string, password: string) => Promise<void>
  loginWithPasskey: (credential: Record<string, unknown>, challenge: string) => Promise<void>
  logout: () => Promise<void>
}
