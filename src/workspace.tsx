import { createContext, useContext, useEffect, useState } from 'react'

import { translations, type TranslationKey } from './i18n'

export function useTranslation() {
  const { settings } = useWorkspace()
  const languageIndex = settings.language === '简体中文' ? 1 : settings.language === '日本語' ? 2 : 0
  return (key: TranslationKey, values?: Record<string, string | number>) => {
    let text: string = translations[key]?.[languageIndex] || translations[key]?.[0] || key
    if (values) Object.entries(values).forEach(([name, value]) => { text = text.replace(`{${name}}`, String(value)) })
    return text
  }
}

export type Page = 'Overview' | 'Providers' | 'Routing' | 'Plugins' | 'Requests' | 'Analytics' | 'Settings'
export type Provider = {
  id: number
  name: string
  short: string
  type: string
  models: number
  latency: number
  success: string
  requests: string
  color: string
  enabled: boolean
  endpoint: string
  group: string
  modelsList: string[]
  routeOnly?: boolean
  fetchModels?: boolean
  modelsFetchedAt?: string
  modelCatalog?: string[]
  keyCount?: number
}
export type ChannelGroup = { id: string; name: string; description: string }
export type RouteCandidate = {
  id: string
  provider: string
  model: string
  enabled: boolean
  weight: number
}
export type RouteRule = {
  id: number
  name: string
  description: string
  match: string
  primary: string
  primaryModel: string
  fallback: string
  fallbackModel: string
  strategy: string
  active: boolean
  requests: string
  slug?: string
  candidates?: RouteCandidate[]
  patterns?: string[]
  listMembers?: boolean
  rateLimit?: string
  noOutput?: string
  allBusy?: string
  context?: string
  contextTokens?: number
  session?: string
}
export type Plugin = {
  id: number
  name: string
  description: string
  version: string
  type: string
  icon: string
  enabled: boolean
  color: string
  author: string
  permissions: string[]
  installed: string
}
export type RequestRecord = {
  id: string
  time: string
  model: string
  provider: string
  tokens: string
  inputTokens: number
  outputTokens: number
  latency: string
  latencyMs: number
  status: string
  key: string
  cost: string
  route: string
}
export type Settings = {
  workspace: string
  language: string
  telemetry: boolean
  updates: boolean
  deployment: 'local' | 'cloud'
  protocol: 'http' | 'https'
  host: string
  port: string
  allowLan: boolean
  requestLogging: boolean
  logBodies: boolean
  timeout: string
  retries: string
  strategy: string
  affinity: boolean
  affinityTtl: string
  circuitBreaker: boolean
  theme: string
  accent: string
  reducedMotion: boolean
  compact: boolean
  requireApproval: boolean
  pluginNetwork: boolean
  pluginSecrets: boolean
}
export type Notice = { id: number; title: string; description: string; time: string; read: boolean }

export const providerModels: Record<string, string[]> = {
  OpenAI: ['gpt-4.1', 'gpt-4.1-mini', 'gpt-4o', 'o3', 'o4-mini'],
  Anthropic: ['claude-sonnet-4', 'claude-opus-4.1', 'claude-haiku-3.5'],
  'Google AI': ['gemini-2.5-pro', 'gemini-2.5-flash', 'gemini-2.0-flash'],
  DeepSeek: ['deepseek-chat', 'deepseek-reasoner'],
  'OpenAI Compatible': ['default'],
}

export const protocolTypes = ['openai', 'anthropic', 'gemini'] as const
export type ProtocolType = typeof protocolTypes[number]
export const defaultChannelGroups: ChannelGroup[] = [
  { id: 'official', name: '官方渠道', description: '官方供应商服务' },
  { id: 'free', name: '免费渠道', description: '免费或共享额度渠道' },
  { id: 'team', name: '团队渠道', description: '团队共享的生产渠道' },
]
export const protocolLabels: Record<ProtocolType, string> = { openai: 'OpenAI 兼容', anthropic: 'Anthropic 兼容', gemini: 'Gemini 兼容' }
export function protocolOf(provider: Pick<Provider, 'type'>): ProtocolType {
  if (provider.type === 'anthropic' || provider.type === 'Anthropic') return 'anthropic'
  if (provider.type === 'gemini' || provider.type === 'Google AI') return 'gemini'
  return 'openai'
}
export function protocolModels(type: ProtocolType) {
  return type === 'anthropic' ? providerModels.Anthropic : type === 'gemini' ? providerModels['Google AI'] : providerModels.OpenAI
}
export const defaultSettings: Settings = {
  workspace: 'Personal workspace', language: 'English', telemetry: false, updates: true,
  deployment: 'local', protocol: 'http', host: '127.0.0.1', port: '3425', allowLan: false, requestLogging: true, logBodies: false,
  timeout: '60', retries: '2', strategy: 'Smart routing', affinity: true, affinityTtl: '30', circuitBreaker: true,
  theme: 'Sepia dark', accent: 'Sage', reducedMotion: false, compact: false,
  requireApproval: true, pluginNetwork: false, pluginSecrets: false,
}
export const defaultProviders: Provider[] = [
  { id: 1, name: 'OpenAI', short: 'O', type: 'openai', group: 'official', models: 12, latency: 542, success: '99.99%', requests: '12.4k', color: '#dcf1d2', enabled: true, endpoint: 'https://api.openai.com/v1', modelsList: providerModels.OpenAI },
  { id: 2, name: 'Anthropic', short: 'A', type: 'anthropic', group: 'official', models: 6, latency: 718, success: '99.98%', requests: '7.8k', color: '#e8cdb7', enabled: true, endpoint: 'https://api.anthropic.com/v1', modelsList: providerModels.Anthropic },
  { id: 3, name: 'Google AI', short: 'G', type: 'gemini', group: 'free', models: 9, latency: 631, success: '99.92%', requests: '3.1k', color: '#c5d9ef', enabled: true, endpoint: 'https://generativelanguage.googleapis.com/v1beta', modelsList: providerModels['Google AI'] },
  { id: 4, name: 'DeepSeek', short: 'D', type: 'openai', group: 'free', models: 4, latency: 892, success: '99.87%', requests: '1.5k', color: '#c8c5ee', enabled: true, endpoint: 'https://api.deepseek.com/v1', modelsList: providerModels.DeepSeek },
]
export const defaultRoutes: RouteRule[] = [
  { id: 1, name: 'Claude, but always available', description: 'Your Claude requests, with a reliable fallback.', match: 'claude-*', primary: 'Anthropic', primaryModel: 'claude-sonnet-4', fallback: 'OpenAI', fallbackModel: 'gpt-4.1', strategy: 'Failover', active: true, requests: '6.8k' },
  { id: 2, name: 'Fast answers, less waiting', description: 'Route everyday conversations to the fastest model.', match: 'gpt-4.1-mini', primary: 'OpenAI', primaryModel: 'gpt-4.1-mini', fallback: 'Google AI', fallbackModel: 'gemini-2.5-flash', strategy: 'Lowest latency', active: true, requests: '11.2k' },
  { id: 3, name: 'Deep thinking, smart spending', description: 'Complex reasoning without unnecessary cost.', match: 'deepseek-*', primary: 'DeepSeek', primaryModel: 'deepseek-reasoner', fallback: 'Anthropic', fallbackModel: 'claude-sonnet-4', strategy: 'Cost optimized', active: true, requests: '2.4k' },
]
export const defaultPlugins: Plugin[] = [
  { id: 1, name: 'Smart Router', description: 'Choose the best model using live latency, cost and health signals.', version: '1.4.0', type: 'Routing', icon: 'route', enabled: true, color: 'mint', author: 'Sepia', permissions: ['Read routing candidates', 'Read provider health'], installed: 'Nov 3, 2026' },
  { id: 2, name: 'Request Inspector', description: 'Understand every decision with detailed, privacy-first request traces.', version: '1.2.1', type: 'Observability', icon: 'activity', enabled: true, color: 'blue', author: 'Sepia', permissions: ['Read request metadata'], installed: 'Nov 3, 2026' },
  { id: 3, name: 'Semantic Cache', description: 'Reuse responses for similar requests and reduce your provider spend.', version: '0.9.3', type: 'Middleware', icon: 'zap', enabled: false, color: 'purple', author: 'Sepia Labs', permissions: ['Read requests', 'Store cached responses'], installed: 'Nov 4, 2026' },
  { id: 4, name: 'Privacy Guard', description: 'Keep sensitive information safely inside your workspace environment.', version: '1.0.2', type: 'Middleware', icon: 'shield', enabled: true, color: 'orange', author: 'Sepia', permissions: ['Transform request metadata'], installed: 'Nov 4, 2026' },
]
export const defaultRequests: RequestRecord[] = [
  ['14:32:08', 'claude-sonnet-4', 'Anthropic', 4821, 642, '200 OK', 'Development'], ['14:31:54', 'gpt-4.1', 'OpenAI', 2304, 481, '200 OK', 'Production'],
  ['14:31:22', 'gemini-2.5-flash', 'Google AI', 1892, 529, '200 OK', 'Development'], ['14:30:47', 'deepseek-reasoner', 'DeepSeek', 8412, 1240, '200 OK', 'Production'],
  ['14:30:12', 'claude-sonnet-4', 'OpenAI', 3219, 702, 'Failover', 'Development'], ['14:29:56', 'gpt-4.1-mini', 'OpenAI', 1084, 319, '200 OK', 'Production'],
  ['14:29:14', 'gemini-2.5-pro', 'Google AI', 5422, 884, '200 OK', 'Development'], ['14:28:49', 'gpt-4.1', 'OpenAI', 3921, 621, '200 OK', 'Production'],
  ['14:28:11', 'claude-opus-4.1', 'Anthropic', 6812, 1112, '200 OK', 'Development'], ['14:27:42', 'deepseek-chat', 'DeepSeek', 2241, 788, '200 OK', 'Production'],
  ['14:26:54', 'gpt-4.1-mini', 'OpenAI', 1491, 402, '200 OK', 'Development'], ['14:26:13', 'gemini-2.5-flash', 'Google AI', 932, 488, '200 OK', 'Production'],
  ['14:25:32', 'claude-sonnet-4', 'Anthropic', 4182, 728, '200 OK', 'Development'], ['14:24:46', 'gpt-4o', 'OpenAI', 2108, 582, '200 OK', 'Production'],
  ['14:23:57', 'deepseek-reasoner', 'DeepSeek', 7381, 1430, '200 OK', 'Development'], ['14:22:19', 'gemini-2.5-pro', 'Google AI', 3089, 812, '200 OK', 'Production'],
  ['14:21:38', 'gpt-4.1-mini', 'OpenAI', 1192, 329, '200 OK', 'Development'], ['14:20:42', 'claude-sonnet-4', 'Anthropic', 5120, 691, '200 OK', 'Production'],
].map((row, index) => {
  const [time, model, provider, tokens, latencyMs, status, key] = row as [string, string, string, number, number, string, string]
  return { id: `req_01J${String(8920 - index)}`, time, model, provider, tokens: tokens.toLocaleString('en-US'), inputTokens: Math.round(tokens * .68), outputTokens: Math.round(tokens * .32), latency: latencyMs > 999 ? `${(latencyMs / 1000).toFixed(2)} s` : `${latencyMs} ms`, latencyMs, status, key, cost: `$${(tokens * .000006).toFixed(4)}`, route: status === 'Failover' ? 'Claude, but always available' : 'Smart routing' }
})
export const defaultNotices: Notice[] = [
  { id: 1, title: 'A smooth recovery', description: 'A request failed over from Anthropic to OpenAI. No interruption.', time: '2 min ago', read: false },
  { id: 2, title: 'Your gateway is up to date', description: 'Sepia v0.1.0 is running with all providers operational.', time: '1 hour ago', read: false },
  { id: 3, title: 'A little saving, automatically', description: 'Smart routing saved an estimated $12.40 this week.', time: 'Yesterday', read: true },
]
export const chartData = [
  ['00:00', 420, 419], ['02:00', 290, 289], ['04:00', 210, 209], ['06:00', 340, 339], ['08:00', 670, 668], ['10:00', 880, 878],
  ['12:00', 770, 769], ['14:00', 1020, 1018], ['16:00', 910, 909], ['18:00', 740, 739], ['20:00', 950, 948], ['22:00', 710, 709], ['23:59', 560, 559],
].map(([time, requests, successful]) => ({ time: String(time), requests: Number(requests), successful: Number(successful) }))

type Workspace = {
  providers: Provider[]; routes: RouteRule[]; plugins: Plugin[]; requests: RequestRecord[]; settings: Settings; notices: Notice[]; channelGroups: ChannelGroup[]
}
type WorkspaceContext = Workspace & {
  setProviders: React.Dispatch<React.SetStateAction<Provider[]>>
  setRoutes: React.Dispatch<React.SetStateAction<RouteRule[]>>
  setPlugins: React.Dispatch<React.SetStateAction<Plugin[]>>
  setRequests: React.Dispatch<React.SetStateAction<RequestRecord[]>>
  setSettings: React.Dispatch<React.SetStateAction<Settings>>
  setNotices: React.Dispatch<React.SetStateAction<Notice[]>>
  setChannelGroups: React.Dispatch<React.SetStateAction<ChannelGroup[]>>
  notify: (message: string) => void
  toast: string
  clearToast: () => void
  resetWorkspace: () => void
}
const WorkspaceCtx = createContext<WorkspaceContext | null>(null)
const STORAGE_KEY = 'sepia.workspace.v1'

function loadWorkspace(): Workspace {
  const fallback = { providers: defaultProviders, routes: defaultRoutes, plugins: defaultPlugins, requests: defaultRequests, settings: defaultSettings, notices: defaultNotices, channelGroups: defaultChannelGroups }
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (!stored) return fallback
    const parsed = JSON.parse(stored)
    return {
      providers: Array.isArray(parsed.providers) ? parsed.providers.map((provider: Provider) => ({ ...provider, type: protocolOf(provider), group: provider.group || 'official' })) : fallback.providers,
      channelGroups: Array.isArray(parsed.channelGroups) && parsed.channelGroups.length ? parsed.channelGroups : fallback.channelGroups,
      routes: Array.isArray(parsed.routes) ? parsed.routes : fallback.routes,
      plugins: Array.isArray(parsed.plugins) ? parsed.plugins : fallback.plugins,
      requests: Array.isArray(parsed.requests) ? parsed.requests : fallback.requests,
      settings: { ...defaultSettings, ...parsed.settings },
      notices: Array.isArray(parsed.notices) ? parsed.notices : fallback.notices,
    }
  } catch { return fallback }
}

export function WorkspaceProvider({ children }: { children: React.ReactNode }) {
  const [initial] = useState(loadWorkspace)
  const [providers, setProviders] = useState(initial.providers)
  const [routes, setRoutes] = useState(initial.routes)
  const [plugins, setPlugins] = useState(initial.plugins)
  const [requests, setRequests] = useState(initial.requests)
  const [settings, setSettings] = useState(initial.settings)
  const [notices, setNotices] = useState(initial.notices)
  const [channelGroups, setChannelGroups] = useState(initial.channelGroups)
  const [toast, setToast] = useState('')
  const notify = (message: string) => setToast(message)
  useEffect(() => {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify({ providers, routes, plugins, requests, settings, notices, channelGroups })) } catch { /* Storage may be disabled by the browser. */ }
  }, [providers, routes, plugins, requests, settings, notices, channelGroups])
  useEffect(() => {
    const colors: Record<string, string> = { Sage: '#c4eaaa', Sky: '#a5cfe5', Lavender: '#c3b3e5', Sand: '#dfc49f' }
    const root = document.documentElement
    root.dataset.theme = settings.theme === 'Light' || (settings.theme === 'System' && window.matchMedia('(prefers-color-scheme: light)').matches) ? 'light' : 'dark'
    root.style.setProperty('--accent', colors[settings.accent] || colors.Sage)
    root.classList.toggle('reduce-motion', settings.reducedMotion)
    root.classList.toggle('compact-tables', settings.compact)
    root.lang = settings.language === '简体中文' ? 'zh-CN' : settings.language === '日本語' ? 'ja' : 'en'
  }, [settings.theme, settings.accent, settings.reducedMotion, settings.compact, settings.language])
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: light)')
    const update = () => { if (settings.theme === 'System') document.documentElement.dataset.theme = media.matches ? 'light' : 'dark' }
    media.addEventListener('change', update)
    return () => media.removeEventListener('change', update)
  }, [settings.theme])
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(''), 3500); return () => clearTimeout(timer) }, [toast])
  const resetWorkspace = () => {
    setProviders(defaultProviders); setRoutes(defaultRoutes); setPlugins(defaultPlugins); setRequests(defaultRequests); setSettings(defaultSettings); setNotices(defaultNotices); setChannelGroups(defaultChannelGroups)
    const index = settings.language === '简体中文' ? 1 : settings.language === '日本語' ? 2 : 0
    notify(translations.demoRestored[index])
  }
  return <WorkspaceCtx.Provider value={{ providers, routes, plugins, requests, settings, notices, channelGroups, setProviders, setRoutes, setPlugins, setRequests, setSettings, setNotices, setChannelGroups, notify, toast, clearToast: () => setToast(''), resetWorkspace }}>{children}</WorkspaceCtx.Provider>
}
export function useWorkspace() {
  const context = useContext(WorkspaceCtx)
  if (!context) throw new Error('useWorkspace must be used within WorkspaceProvider')
  return context
}
export function downloadFile(name: string, content: string, type = 'application/json') {
  const url = URL.createObjectURL(new Blob([content], { type }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = name
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 100)
}
