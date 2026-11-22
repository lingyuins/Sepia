import type { Settings } from './workspace'

export const defaultSettings: Settings = {
  workspace: 'Personal workspace', language: 'English', telemetry: false, updates: true,
  deployment: 'local', protocol: 'http', host: '127.0.0.1', port: '5000', allowLan: false, requestLogging: true, logBodies: false,
  retries: '3', connectTimeout: '10', firstByteTimeout: '60', streamIdleTimeout: '60', retryAfterCap: '5',
  strategy: 'Failover', affinity: false, affinityTtl: '30', circuitBreaker: false,
  theme: 'Sepia dark', accent: 'Sage', reducedMotion: false, compact: false,
  requireApproval: true, pluginNetwork: false, pluginSecrets: false,
  normalizePrefixes: [], normalizeSuffixes: [], normalizeMappings: [],
  aiPoolEnabled: false,
}
