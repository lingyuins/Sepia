import {
  ArrowDownToLine,
  ArrowRight,
  ArrowUpFromLine,
  Brain,
  ChevronRight,
  Clock,
  Cpu,
  DollarSign,
  Gauge,
  Globe,
  KeyRound,
  Percent,
  Sigma,
  Zap,
} from 'lucide-react'
import { useTranslation, useLocale, type RequestRecord } from '../workspace'
import { formatRequestTime } from '../analytics'
import { cacheHitRate, formatTps } from './RequestDetails'
import { ModelAvatar, getModelBrand, resolveBrandColor } from '../model-brand'

// Octopus-style log card: brand avatar + requested model → endpoint → channel
// chain, then a compact colored metric grid (ported from Octopus LogCard).
// Only recorded values are shown — no invented telemetry.

const endpointMeta: Record<string, { label: 'logEndpointChatFull' | 'logEndpointMessages' | 'logEndpointResponses' | 'logEndpointGemini'; color: string }> = {
  openai: { label: 'logEndpointChatFull', color: '#10a37f' },
  'openai-responses': { label: 'logEndpointResponses', color: '#0ea5e9' },
  anthropic: { label: 'logEndpointMessages', color: '#d7765a' },
  gemini: { label: 'logEndpointGemini', color: '#4285f4' },
}

function formatCost(cost: string): string {
  const value = Number(cost.replace('$', ''))
  if (!Number.isFinite(value)) return cost
  if (value === 0) return '$0.00'
  if (value >= 1) return `$${value.toFixed(2)}`
  return `$${Number(value.toPrecision(2))}`
}

// Compact K/M/B counts (matches the reference screenshot; the original's
// 万/亿 mode is a separate chinaMode setting Sepia does not carry).
function formatCount(value: number, locale: string): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(2)}B`
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`
  if (value >= 1_000) return `${(value / 1_000).toFixed(2)}K`
  return value.toLocaleString(locale)
}

function formatLogTime(value: string, locale: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return `${date.toLocaleDateString(locale, { month: 'numeric', day: 'numeric' })} ${formatRequestTime(value, locale)}`
}

export function RequestLogCards({
  rows,
  onSelect,
  isDark,
}: {
  rows: RequestRecord[]
  onSelect: (row: RequestRecord) => void
  isDark: boolean
}) {
  const t = useTranslation()
  const locale = useLocale()
  const number = (value: number) => formatCount(value, locale)

  if (!rows.length) {
    return <div className="log-cards-empty">{t('noRequestMatch')}</div>
  }

  return (
    <div className="log-cards">
      {rows.map((row) => {
        const brand = getModelBrand(row.requestedModel)
        const brandColor = resolveBrandColor(brand.color, isDark)
        const endpoint = endpointMeta[row.protocol] || endpointMeta.openai
        const failed = row.status === 'Failed'
        // Anthropic usage counts exclude cached tokens; OpenAI/Gemini include them.
        const realInput =
          row.protocol === 'anthropic'
            ? row.inputTokens
            : Math.max(0, row.inputTokens - row.cachedTokens)
        // Total = real input + cache reads + output under BOTH protocol conventions.
        const totalTokens = realInput + row.cachedTokens + row.outputTokens
        const metrics: { icon: typeof Clock; color: string; text: string; title: string; strong?: string }[] = [
          { icon: Clock, color: brandColor, text: formatLogTime(row.createdAt, locale), title: t('time') },
          { icon: KeyRound, color: '#f97316', text: row.key, title: t('apiKey') },
          ...(row.clientIp ? [{ icon: Globe, color: '#0ea5e9', text: row.clientIp, title: t('clientIp') }] : []),
          ...(row.firstByteMs !== null ? [{ icon: Zap, color: '#f59e0b', text: `${t('logFirstByte')} ${row.firstByteMs < 1000 ? `${row.firstByteMs}ms` : `${(row.firstByteMs / 1000).toFixed(2)}s`}`, title: t('firstByte') }] : []),
          { icon: Cpu, color: '#3b82f6', text: `${t('logTotalTime')} ${row.latencyMs < 1000 ? `${row.latencyMs}ms` : `${(row.latencyMs / 1000).toFixed(2)}s`}`, title: t('latency') },
          { icon: Gauge, color: '#84cc16', text: `${t('logTps')} ${formatTps(row)}`, title: t('tps') },
          ...(row.cachedTokens > 0 ? [{ icon: Percent, color: '#14b8a6', text: `${t('cacheHitRate')} ${cacheHitRate(row)}`, title: t('cacheHitRate') }] : []),
          { icon: ArrowDownToLine, color: '#22c55e', text: `${row.cachedTokens > 0 ? t('logRealInput') : t('logInput')} ${number(realInput)}`, title: row.cachedTokens > 0 ? t('logRealInput') : t('logInput') },
          ...(row.cachedTokens > 0 ? [{ icon: ArrowDownToLine, color: '#14b8a6', text: `${t('cachedTokens')} ${number(row.cachedTokens)}`, title: t('cachedTokens') }] : []),
          { icon: ArrowUpFromLine, color: '#a855f7', text: `${t('logOutput')} ${number(row.outputTokens)}`, title: t('logOutput') },
          { icon: Sigma, color: '#f43f5e', text: `${t('totalTokens')} ${number(totalTokens)}`, title: t('totalTokens'), strong: 'log-metric-rose' },
          { icon: DollarSign, color: '#10b981', text: `${t('logCost')} ${formatCost(row.cost)}`, title: t('estimatedCost'), strong: 'log-metric-emerald' },
          ...(row.reasoningTokens > 0 ? [{ icon: Brain, color: '#6366f1', text: `${t('logReasoning')} ${number(row.reasoningTokens)}${t('logReasoningUnit')}`, title: t('reasoningTokens') }] : []),
        ]
        return (
          <article
            key={row.id}
            className={`log-card ${failed ? 'log-card-error' : ''}`}
            onClick={() => onSelect(row)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault()
                onSelect(row)
              }
            }}
            role="button"
            tabIndex={0}
            aria-label={t('requestDetails')}
          >
            <div className="log-card-head">
              <ModelAvatar model={row.requestedModel} size={36} isDark={isDark} />
              <div className="log-card-chain">
                <strong className="mono" title={row.requestedModel}>{row.requestedModel}</strong>
                <ArrowRight size={13} aria-hidden="true" />
                <span className="log-badge" style={{ background: `${endpoint.color}1f`, color: endpoint.color }}>
                  {t(endpoint.label)}
                </span>
                {row.provider && (
                  <>
                    <ArrowRight size={13} aria-hidden="true" />
                    <span className="log-badge" style={{ background: `${brandColor}1f`, color: brandColor }} title={row.provider}>
                      {row.provider}
                    </span>
                  </>
                )}
                {row.model !== row.requestedModel && row.model !== 'unknown' && (
                  <span className="log-card-resolved mono" title={row.model}>{row.model}</span>
                )}
                <ChevronRight size={15} className="log-card-open" aria-hidden="true" />
              </div>
            </div>
            <div className="log-card-metrics tabular">
              {metrics.map((metric, index) => (
                <span key={index} className={`log-metric ${metric.strong || ''}`} title={metric.title}>
                  <metric.icon size={13} style={{ color: metric.color }} aria-hidden="true" />
                  <span className="log-metric-text">{metric.text}</span>
                </span>
              ))}
            </div>
            {row.error && <p className="log-card-error-text mono">{row.error}</p>}
          </article>
        )
      })}
    </div>
  )
}