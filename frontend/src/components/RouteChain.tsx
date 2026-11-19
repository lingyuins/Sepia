import { ArrowRight, GitBranch, KeyRound } from 'lucide-react'
import { useTranslation, useWorkspace, type RequestRecord, type RouteRule } from '../workspace'
import { routeDisplayCandidates } from '../route-display'
import { ProviderCell, StatusDot } from './UI'
import { strategyLabelKey } from '../route-strategy'

export function RouteChain({ route, latestRequest, compact = false }: { route: RouteRule; latestRequest?: RequestRecord; compact?: boolean }) {
  const t = useTranslation()
  const { providers } = useWorkspace()
  const candidates = routeDisplayCandidates(route)
  return <div className={`route-chain ${compact ? 'compact' : ''}`}>
    <div className="route-chain-node route-chain-entry"><small><KeyRound size={13} />{t('routeEntry')}</small><strong>{route.slug || route.name}</strong><span>{t('clientRouteId')}</span></div>
    {candidates.map((candidate, index) => <div className="route-chain-segment" key={candidate.id}>
      <div className="route-chain-connector"><ArrowRight size={16} /><span>{index === 0 ? t(strategyLabelKey(route.strategy)) : t('nextCandidate')}</span></div>
      <div className={`route-chain-node ${providers.find((provider) => provider.name === candidate.provider)?.enabled === false ? 'is-unavailable' : ''}`}><small><GitBranch size={13} />{String(index + 1).padStart(2, '0')} · {index === 0 ? t('primary') : t('candidateNumber', { number: index + 1 })}</small><ProviderCell name={candidate.provider} /><code title={candidate.model}>{candidate.model}</code>{providers.find((provider) => provider.name === candidate.provider)?.enabled === false && <span className="route-candidate-unavailable">{t('providerUnavailable')}</span>}</div>
    </div>)}
    {latestRequest && <div className="route-chain-segment route-chain-result"><div className="route-chain-connector"><ArrowRight size={16} /><span>{t('latestSuccess')}</span></div><div className="route-chain-node"><small><StatusDot />{t('actualModel')}</small><strong>{latestRequest.model}</strong><span>{latestRequest.status} · {latestRequest.time}</span></div></div>}
  </div>
}
