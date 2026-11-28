import { useState } from 'react'
import {
  Activity,
  ArrowDown,
  ArrowUp,
  ChevronDown,
  Copy,
  GripVertical,
  Pencil,
  Trash2,
  X,
} from 'lucide-react'
import { useTranslation, type RouteRule } from '../workspace'
import { StatusText, Toggle } from './UI'
import { routeDisplayCandidates } from '../route-display'
import {
  ROUTE_STRATEGY_IDS,
  strategyLabelKey,
  strategyUsesWeights,
  clampWeight,
} from '../route-strategy'
import { ModelAvatar } from '../model-brand'

// Octopus-style collapsible route cards: collapsed header shows the first
// candidate's brand avatar, strategy and member count; expanding reveals the
// strategy switcher and the ordered candidate list. Every edit persists
// immediately through the normal route API (same as the Octopus group card).
export function RouteCards({
  routes,
  isDark,
  onToggle,
  onEdit,
  onDelete,
  onTest,
  onCopy,
  onPersist,
}: {
  routes: RouteRule[]
  isDark: boolean
  onToggle: (route: RouteRule, enabled: boolean) => void
  onEdit: (route: RouteRule) => void
  onDelete: (route: RouteRule) => void
  onTest: (route: RouteRule) => void
  onCopy: (route: RouteRule) => void
  onPersist: (route: RouteRule, patch: Partial<RouteRule>) => void
}) {
  const t = useTranslation()
  const [expandedId, setExpandedId] = useState<number | null>(null)

  return (
    <div className="route-cards">
      {routes.map((route) => {
        const candidates = routeDisplayCandidates(route)
        const expanded = expandedId === route.id
        const firstModel = candidates[0]?.model || route.slug || route.name
        const strategyName = t(strategyLabelKey(route.strategy))
        const move = (index: number, direction: -1 | 1) => {
          const target = index + direction
          if (target < 0 || target >= candidates.length) return
          const next = [...candidates]
          const [candidate] = next.splice(index, 1)
          next.splice(target, 0, candidate)
          onPersist(route, { candidates: next })
        }
        return (
          <article key={route.id} className={`route-card ${route.active ? '' : 'route-card-paused'}`}>
            <div
              className="route-card-head"
              role="button"
              tabIndex={0}
              aria-expanded={expanded}
              onClick={() => setExpandedId(expanded ? null : route.id)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  setExpandedId(expanded ? null : route.id)
                }
              }}
            >
              <ModelAvatar model={firstModel} size={36} isDark={isDark} />
              <div className="route-card-title">
                <strong title={route.name}>{route.name}</strong>
                <span className="route-card-sub">
                  {strategyName} · <span className="mono">{firstModel}</span> ·{' '}
                  {t('candidatesCount', { count: candidates.length })}
                </span>
              </div>
              <StatusText tone={route.active ? 'positive' : 'neutral'}>
                {route.active ? t('routeActive') : t('routePaused')}
              </StatusText>
              <button
                type="button"
                className="icon-button"
                aria-label={t('testRoute')}
                title={t('testRoute')}
                onClick={(event) => {
                  event.stopPropagation()
                  onTest(route)
                }}
              >
                <Activity size={15} />
              </button>
              <ChevronDown size={16} className={`route-card-chevron ${expanded ? 'open' : ''}`} aria-hidden="true" />
            </div>
            {expanded && (
              <div className="route-card-body">
                <div className="route-card-strategies" role="group" aria-label={t('routingStrategy')}>
                  {ROUTE_STRATEGY_IDS.map((id) => {
                    const label = t(strategyLabelKey(id))
                    return (
                      <button
                        key={id}
                        type="button"
                        title={label}
                        className={route.strategy === id ? 'selected' : ''}
                        onClick={() => {
                          if (route.strategy !== id) onPersist(route, { strategy: id })
                        }}
                      >
                        {label}
                      </button>
                    )
                  })}
                </div>
                <ol className="route-card-candidates">
                  {candidates.map((candidate, index) => (
                    <li
                      key={candidate.id}
                      onDragOver={(event) => event.preventDefault()}
                      onDrop={(event) => {
                        event.preventDefault()
                        const source = Number(event.dataTransfer.getData('text/plain'))
                        if (!Number.isInteger(source) || source === index || source < 0 || source >= candidates.length) return
                        const next = [...candidates]
                        const [moved] = next.splice(source, 1)
                        next.splice(index, 0, moved)
                        onPersist(route, { candidates: next })
                      }}
                    >
                      <span className="route-card-index tabular">{index + 1}</span>
                      <span
                        className="route-card-grip"
                        draggable
                        onDragStart={(event) => event.dataTransfer.setData('text/plain', String(index))}
                        aria-hidden="true"
                      >
                        <GripVertical size={14} />
                      </span>
                      <ModelAvatar model={candidate.model} size={22} isDark={isDark} />
                      <span className="route-card-model mono" title={candidate.model}>{candidate.model}</span>
                      <span className="route-card-provider" title={candidate.provider}>{candidate.provider}</span>
                      {strategyUsesWeights(route.strategy) && (
                        <input
                          key={`${candidate.id}-${candidate.weight}`}
                          type="number"
                          min={1}
                          max={100}
                          defaultValue={candidate.weight}
                          className="route-card-weight tabular"
                          aria-label={t('candidateWeightNamed', { name: candidate.model })}
                          onBlur={(event) => {
                            const weight = clampWeight(Number(event.target.value) || 1)
                            if (weight !== candidate.weight) {
                              onPersist(route, {
                                candidates: candidates.map((item) =>
                                  item.id === candidate.id ? { ...item, weight } : item,
                                ),
                              })
                            }
                          }}
                        />
                      )}
                      <span className="route-card-row-actions">
                        <button
                          type="button"
                          className="icon-button"
                          disabled={index === 0}
                          title={t('moveCandidateUp', { name: candidate.model })}
                          aria-label={t('moveCandidateUp', { name: candidate.model })}
                          onClick={() => move(index, -1)}
                        >
                          <ArrowUp size={13} />
                        </button>
                        <button
                          type="button"
                          className="icon-button"
                          disabled={index === candidates.length - 1}
                          title={t('moveCandidateDown', { name: candidate.model })}
                          aria-label={t('moveCandidateDown', { name: candidate.model })}
                          onClick={() => move(index, 1)}
                        >
                          <ArrowDown size={13} />
                        </button>
                        <button
                          type="button"
                          className="icon-button"
                          title={t('removeCandidate', { name: candidate.model })}
                          aria-label={t('removeCandidate', { name: candidate.model })}
                          onClick={() =>
                            onPersist(route, { candidates: candidates.filter((item) => item.id !== candidate.id) })
                          }
                        >
                          <X size={13} />
                        </button>
                      </span>
                    </li>
                  ))}
                </ol>
                {candidates.length === 0 && (
                  <p className="route-card-empty">{t('routeNeedsModel')}</p>
                )}
                <div className="route-card-foot">
                  <Toggle
                    checked={route.active}
                    onChange={() => onToggle(route, !route.active)}
                    label={t('toggleNamed', { name: route.name })}
                  />
                  <span className="route-card-foot-actions">
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t('editNamed', { name: route.name })}
                      onClick={() => onEdit(route)}
                    >
                      <Pencil size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t('copyRouteName')}
                      onClick={() => onCopy(route)}
                    >
                      <Copy size={15} />
                    </button>
                    <button
                      type="button"
                      className="icon-button danger-icon"
                      aria-label={t('deleteNamed', { name: route.name })}
                      onClick={() => onDelete(route)}
                    >
                      <Trash2 size={15} />
                    </button>
                  </span>
                </div>
              </div>
            )}
          </article>
        )
      })}
    </div>
  )
}