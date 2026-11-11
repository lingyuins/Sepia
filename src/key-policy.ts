export type ModelQuota = { rpm: number; tpm: number }
export type KeyPolicy = {
  name: string
  customValue: string
  tags: string[]
  maxAmount: number | null
  tokenLimit: number | null
  rpm: number
  tpm: number
  modelQuotas: Record<string, ModelQuota>
  ipWhitelist: string[]
  expiresAt: string | null
  models: string[]
  allowedGroups: string[]
  excludedProviders: string[]
  enabled: boolean
}
export type KeyPolicyDraft = Omit<KeyPolicy, 'maxAmount' | 'tokenLimit' | 'rpm' | 'tpm' | 'modelQuotas' | 'ipWhitelist' | 'expiresAt'> & {
  maxAmount: string
  tokenLimit: string
  rpm: string
  tpm: string
  modelQuotas: string
  ipWhitelist: string
  expiresAt: string
}

export function parseTokenLimit(value: string) {
  const normalized = value.trim().toLowerCase().replace(/,/g, '')
  if (!normalized) return null
  const match = normalized.match(/^(\d+(?:\.\d+)?)(w|万|亿)?$/)
  if (!match) throw new Error('Invalid Token limit')
  return Math.round(Number(match[1]) * (match[2] === '亿' ? 100000000 : match[2] ? 10000 : 1))
}

function nonNegative(value: string, label: string) {
  const parsed = Number(value)
  if (value.trim() === '' || !Number.isFinite(parsed) || parsed < 0) throw new Error(`Invalid ${label}`)
  return parsed
}

function validNetwork(value: string) {
  const [address, prefix, extra] = value.split('/')
  let version = 0
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(address)) {
    version = address.split('.').every((part) => Number(part) <= 255) ? 4 : 0
  } else if (address.includes(':') && /^[\da-f:]+$/i.test(address)) {
    try {
      const hostname = new URL(`http://[${address}]/`).hostname
      version = hostname.startsWith('[') && hostname.endsWith(']') ? 6 : 0
    } catch { version = 0 }
  }
  if (!version || extra !== undefined) return false
  if (prefix === undefined) return true
  return /^\d+$/.test(prefix) && Number(prefix) <= (version === 4 ? 32 : 128)
}

export function parseKeyPolicy(draft: KeyPolicyDraft): KeyPolicy {
  const quotas = draft.modelQuotas.trim() ? JSON.parse(draft.modelQuotas) : {}
  if (!quotas || typeof quotas !== 'object' || Array.isArray(quotas)) throw new Error('Invalid model quota JSON')
  for (const quota of Object.values(quotas) as ModelQuota[]) {
    if (!quota || typeof quota !== 'object' || !Number.isFinite(quota.rpm) || !Number.isFinite(quota.tpm) || quota.rpm < 0 || quota.tpm < 0) throw new Error('Invalid model quota JSON')
  }
  const ipWhitelist = draft.ipWhitelist.split(',').map((entry) => entry.trim()).filter(Boolean)
  if (ipWhitelist.some((entry) => !validNetwork(entry))) throw new Error('Invalid IP whitelist')
  if (/\s/.test(draft.customValue.trim())) throw new Error('Invalid custom key')
  if (draft.expiresAt && !Number.isFinite(Date.parse(draft.expiresAt))) throw new Error('Invalid expiration date')
  return {
    ...draft,
    name: draft.name.trim(),
    customValue: draft.customValue.trim(),
    maxAmount: draft.maxAmount.trim() ? nonNegative(draft.maxAmount, 'maximum amount') : null,
    tokenLimit: parseTokenLimit(draft.tokenLimit),
    rpm: nonNegative(draft.rpm, 'RPM'),
    tpm: nonNegative(draft.tpm, 'TPM'),
    modelQuotas: quotas,
    ipWhitelist,
    expiresAt: draft.expiresAt || null,
  }
}
