import { createContext, useContext } from 'react'
import { useGatewayWorkspace, type GatewayWorkspace } from './gateway-workspace'

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
  isDefault?: boolean
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
  createdAt: string
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
export const providerModels: Record<string, string[]> = {
  OpenAI: ['gpt-4.1', 'gpt-4.1-mini', 'gpt-4o', 'o3', 'o4-mini'],
  Anthropic: ['claude-sonnet-4', 'claude-opus-4.1', 'claude-haiku-3.5'],
  'Google AI': ['gemini-2.5-pro', 'gemini-2.5-flash', 'gemini-2.0-flash'],
  DeepSeek: ['deepseek-chat', 'deepseek-reasoner'],
  'OpenAI Compatible': ['default'],
}

export const protocolTypes = ['openai', 'anthropic', 'gemini', 'openai-responses'] as const
export type ProtocolType = typeof protocolTypes[number]
export const protocolLabels: Record<ProtocolType, string> = { openai: 'OpenAI 兼容', anthropic: 'Anthropic 兼容', gemini: 'Gemini 兼容', 'openai-responses': 'OpenAI Responses' }
export function protocolOf(provider: Pick<Provider, 'type'>): ProtocolType {
  if (provider.type === 'anthropic' || provider.type === 'Anthropic') return 'anthropic'
  if (provider.type === 'gemini' || provider.type === 'Google AI') return 'gemini'
  if (provider.type === 'openai-responses') return 'openai-responses'
  return 'openai'
}
export function protocolModels(type: ProtocolType) {
  return type === 'anthropic' ? providerModels.Anthropic : type === 'gemini' ? providerModels['Google AI'] : providerModels.OpenAI
}

const WorkspaceCtx = createContext<GatewayWorkspace | null>(null)

export function WorkspaceProvider({ children }: { children: React.ReactNode }) {
  const workspace = useGatewayWorkspace()
  return <WorkspaceCtx.Provider value={workspace}>{children}</WorkspaceCtx.Provider>
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
