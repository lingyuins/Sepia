import { useEffect, useState } from 'react'
import { ArrowRight, GitBranch, KeyRound } from 'lucide-react'
import {
  useTranslation,
  useWorkspace,
  useLocale,
  type AttemptRecord,
  type RequestRecord,
  type RouteRule,
} from '../workspace'
import { formatRequestTime } from '../analytics'
import { routeDisplayCandidates } from '../route-display'
import { api, mapAttempt, type ApiAttempt } from '../api'
import { ProviderCell, StatusText } from './UI'
import { strategyLabelKey } from '../route-strategy'

export function RouteChain({
  route,
  latestRequest,
  compact = false,
}: {
  route: RouteRule
  latestRequest?: RequestRecord
  compact?: boolean
}) {
  const t = useTranslation()
  const locale = useLocale()
  const { providers } = useWorkspace()
  const candidates = routeDisplayCandidates(route)
  const [attempts, setAttempts] = useState<{ requestId: string; records: AttemptRecord[] } | null>(
    null,
  )

  useEffect(() => {
    if (!latestRequest) {
      setAttempts(null)
      return
    }
    const requestId = latestRequest.id
    let cancelled = false
    setAttempts(null)
    api<ApiAttempt[]>(`/api/v1/requests/${requestId}/attempts`)
      .then((records) => {
        if (!cancelled) setAttempts({ requestId, records: records.map(mapAttempt) })
      })
      .catch(() => {
        if (!cancelled) setAttempts({ requestId, records: [] })
      })
    return () => {
      cancelled = true
    }
  }, [latestRequest?.id])

  const hopAttempts =
    latestRequest && attempts?.requestId === latestRequest.id ? attempts.records : []

  const hopSummary = (providerName: string, model: string) => {
    const providerId = providers.find((provider) => provider.name === providerName)?.id
    const hops = hopAttempts.filter(
      (attempt) => attempt.providerId === providerId && attempt.model === model,
    )
    if (!hops.length) return null
    const succeeded = hops.some(
      (attempt) => attempt.status === 'success' || attempt.status === 'streaming',
    )
    const retried = hops.some((attempt) => attempt.status === 'retry')
    const last = hops[hops.length - 1]
    return { succeeded, retried, latencyMs: last.latencyMs, count: hops.length }
  }

  return (
    <div className={`route-chain ${compact ? 'compact' : ''}`}>
      <div className="route-chain-node route-chain-entry">
        <small>
          <KeyRound size={13} />
          {t('routeEntry')}
        </small>
        <strong className="mono">{route.slug || route.name}</strong>
        <span>{t('clientRouteId')}</span>
      </div>

      {candidates.map((candidate, index) => {
        const provider = providers.find((item) => item.name === candidate.provider)
        const hop = hopSummary(candidate.provider, candidate.model)
        return (
          <div className="route-chain-segment" key={candidate.id}>
            <div className="route-chain-connector">
              <ArrowRight size={15} />
              <span>{index === 0 ? t(strategyLabelKey(route.strategy)) : t('nextCandidate')}</span>
            </div>
            <div
              className={`route-chain-node ${provider?.enabled === false ? 'is-unavailable' : ''}`}
            >
              <small className="tabular">
                <GitBranch size={13} />
                {String(index + 1).padStart(2, '0')} ·{' '}
                {index === 0 ? t('primary') : t('candidateNumber', { number: index + 1 })}
              </small>
              <ProviderCell name={candidate.provider} />
              <code className="mono" title={candidate.model}>
                {candidate.model}
              </code>
              {hop && (
                <span
                  className={`route-hop tabular ${
                    hop.succeeded ? 'positive' : hop.retried ? '' : 'danger-text'
                  }`}
                  title={t('hopAttempts', { count: hop.count })}
                >
                  {hop.succeeded ? t('hopSuccess') : hop.retried ? t('hopRetry') : t('hopFailed')} ·{' '}
                  {hop.latencyMs} ms
                </span>
              )}
              {provider?.enabled === false && (
                <span className="route-candidate-unavailable">{t('providerUnavailable')}</span>
              )}
            </div>
          </div>
        )
      })}

      {latestRequest && (
        <div className="route-chain-segment route-chain-result">
          <div className="route-chain-connector">
            <ArrowRight size={15} />
            <span>{t('latestSuccess')}</span>
          </div>
          <div className="route-chain-node">
            <small>
              <StatusText tone="positive">{t('actualModel')}</StatusText>
            </small>
            <strong className="mono">{latestRequest.model}</strong>
            <span className="tabular">
              {latestRequest.status} · {formatRequestTime(latestRequest.createdAt, locale)}
            </span>
          </div>
        </div>
      )}
    </div>
  )
}
