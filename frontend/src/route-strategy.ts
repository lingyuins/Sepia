import type { TranslationKey } from './i18n'

// Wire ids mirror backend/src/strategy.rs STRATEGY_IDS. Unknown values fall
// back to priority so legacy records and plugin-ordered routes keep working.
export const ROUTE_STRATEGY_IDS = [
  'priority', 'weighted', 'round-robin', 'random', 'p2c',
  'least-used', 'cost-optimized', 'latency', 'context-fit', 'sticky',
] as const

export type RouteStrategyId = typeof ROUTE_STRATEGY_IDS[number]

export function isRouteStrategy(value: string): value is RouteStrategyId {
  return (ROUTE_STRATEGY_IDS as readonly string[]).includes(value)
}

export function normalizeRouteStrategy(value: string | undefined): RouteStrategyId {
  if (!value) return 'priority'
  if (isRouteStrategy(value)) return value
  // Legacy UI labels mapped onto the built-in strategies.
  if (value === 'Failover' || value === 'Smart routing' || value === 'Sequential') return 'priority'
  if (value === 'Weighted') return 'weighted'
  if (value === 'Round robin') return 'round-robin'
  if (value === 'Least used') return 'least-used'
  if (value === 'Cost optimized') return 'cost-optimized'
  if (value === 'Lowest latency') return 'latency'
  return 'priority'
}

export const STRATEGY_LABEL_KEYS: Record<RouteStrategyId, TranslationKey> = {
  priority: 'strategyPriority',
  weighted: 'strategyWeighted',
  'round-robin': 'strategyRoundRobin',
  random: 'strategyRandom',
  p2c: 'strategyP2c',
  'least-used': 'strategyLeastUsed',
  'cost-optimized': 'strategyCostOptimized',
  latency: 'strategyLatency',
  'context-fit': 'strategyContextFit',
  sticky: 'strategySticky',
}

export const STRATEGY_DESCRIPTION_KEYS: Record<RouteStrategyId, TranslationKey> = {
  priority: 'strategyPriorityDescription',
  weighted: 'strategyWeightedDescription',
  'round-robin': 'strategyRoundRobinDescription',
  random: 'strategyRandomDescription',
  p2c: 'strategyP2cDescription',
  'least-used': 'strategyLeastUsedDescription',
  'cost-optimized': 'strategyCostOptimizedDescription',
  latency: 'strategyLatencyDescription',
  'context-fit': 'strategyContextFitDescription',
  sticky: 'strategyStickyDescription',
}

export function strategyLabelKey(value: string | undefined): TranslationKey {
  return STRATEGY_LABEL_KEYS[normalizeRouteStrategy(value)]
}

export function strategyDescriptionKey(value: string | undefined): TranslationKey {
  return STRATEGY_DESCRIPTION_KEYS[normalizeRouteStrategy(value)]
}

// Only weighted sampling consumes per-candidate weights; every other strategy
// orders candidates by its own signal and ignores them.
export function strategyUsesWeights(value: string | undefined): boolean {
  return normalizeRouteStrategy(value) === 'weighted'
}

export function clampWeight(value: number): number {
  if (!Number.isFinite(value)) return 1
  return Math.min(100, Math.max(1, Math.round(value)))
}