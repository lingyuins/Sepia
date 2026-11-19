import { useEffect, useState } from 'react'
import { ArrowRight, GitBranch, KeyRound } from 'lucide-react'
import { useTranslation, useWorkspace, type AttemptRecord, type RequestRecord, type RouteRule } from '../workspace'
import { routeDisplayCandidates } from '../route-display'
import { api, mapAttempt, type ApiAttempt } from '../api'
import { ProviderCell, StatusDot } from './UI'
import { strategyLabelKey } from '../route-strategy'

export function RouteChain({ route, latestRequest, compact = false }: { route: RouteRule; latestRequest?: RequestRecord; compact?: boolean }) {
  const t = useTranslation()
  const { providers } = useWorkspace()
  const candidates = routeDisplayCandidates(route)
  const [attempts, setAttempts] = useState<AttemptRecord[]>([])
  useEffect(() => {
    if (!latestRequest) { setAttempts([]); return }
    let cancelled = false
    api<ApiAttempt[]>(`/api/v1/requests/${latestRequest.id}/attempts`)
      .then((records) => { if (!cancelled) setAttempts(records.map(mapAttempt)) })
      .catch(() => { if (!cancelled) setAttempts([]) })
    return () => { cancelled = true }
  }, [latestRequest?.id])
  const hopSummary = (providerName: string, model: string) => {
    const providerId = providers.find((provider) => provider.name === providerName)?.id
    const hops = attempts.filter((attempt) => attempt.providerId === providerId && attempt.model === model)
    if (!hops.length) return null
    const succeeded = hops.some((attempt) => attempt.status === 'success' || attempt.status === 'streaming')
    const retried = hops.some((attempt) => attempt.status === 'retry')
    const last = hops[hops.length - 1]
    return { succeeded, retried, latencyMs: last.latencyMs, count: hops.length }
  }
  return <div className={`route-chain ${compact ? 'compact' : ''}`}>
    <div className="route-chain-node route-chain-entry"><small><KeyRound size={13} />{t('routeEntry')}</small><strong>{route.slug || route.name}</strong><span>{t('clientRouteId')}</span></div>
    {candidates.map((candidate, index) => {
      const provider = providers.find((item) => item.name === candidate.provider)
      const hop = hopSummary(candidate.provider, candidate.model)
      return <div className="route-chain-segment" key={candidate.id}>
        <div className="route-chain-connector"><ArrowRight size={16} /><span>{index === 0 ? t(strategyLabelKey(route.strategy)) : t('nextCandidate')}</span></div>
        <div className={`route-chain-node ${provider?.enabled === false ? 'is-unavailable' : ''}`}><small><GitBranch size={13} />{String(index + 1).padStart(2, '0')} · {index === 0 ? t('primary') : t('candidateNumber', { number: index + 1 })}</small><ProviderCell name={candidate.provider} /><code title={candidate.model}>{candidate.model}</code>{hop && <span className={`route-hop ${hop.succeeded ? 'positive' : hop.retried ? '' : 'danger-text'}`} title={t('hopAttempts', { count: hop.count })}>{hop.succeeded ? t('hopSuccess') : hop.retried ? t('hopRetry') : t('hopFailed')} · {hop.latencyMs} ms</span>}{provider?.enabled === false && <span className="route-candidate-unavailable">{t('providerUnavailable')}</span>}</div>
      </div>
    })}
    {latestRequest && <div className="route-chain-segment route-chain-result"><div className="route-chain-connector"><ArrowRight size={16} /><span>{t('latestSuccess')}</span></div><div className="route-chain-node"><small><StatusDot />{t('actualModel')}</small><strong>{latestRequest.model}</strong><span>{latestRequest.status} · {latestRequest.time}</span></div></div>}
  </div>
}
