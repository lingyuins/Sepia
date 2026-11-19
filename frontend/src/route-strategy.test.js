import assert from 'node:assert/strict'
import test from 'node:test'
import { ROUTE_STRATEGY_IDS, clampWeight, isRouteStrategy, normalizeRouteStrategy, strategyDescriptionKey, strategyLabelKey, strategyUsesWeights } from './route-strategy.ts'

test('exposes exactly the gateway strategy ids', () => {
  assert.deepEqual([...ROUTE_STRATEGY_IDS], ['priority', 'weighted', 'round-robin', 'random', 'p2c', 'least-used', 'cost-optimized', 'latency', 'context-fit', 'sticky'])
  assert.ok(ROUTE_STRATEGY_IDS.every(isRouteStrategy))
  assert.equal(isRouteStrategy('fusion'), false)
})

test('unknown and legacy strategy values fall back to priority', () => {
  assert.equal(normalizeRouteStrategy(undefined), 'priority')
  assert.equal(normalizeRouteStrategy(''), 'priority')
  assert.equal(normalizeRouteStrategy('fusion'), 'priority')
  assert.equal(normalizeRouteStrategy('Failover'), 'priority')
  assert.equal(normalizeRouteStrategy('Smart routing'), 'priority')
  assert.equal(normalizeRouteStrategy('Sequential'), 'priority')
})

test('legacy UI labels map onto their built-in strategies', () => {
  assert.equal(normalizeRouteStrategy('Weighted'), 'weighted')
  assert.equal(normalizeRouteStrategy('Round robin'), 'round-robin')
  assert.equal(normalizeRouteStrategy('Least used'), 'least-used')
  assert.equal(normalizeRouteStrategy('Cost optimized'), 'cost-optimized')
  assert.equal(normalizeRouteStrategy('Lowest latency'), 'latency')
  assert.equal(normalizeRouteStrategy('sticky'), 'sticky')
})

test('every strategy has a distinct label and description key', () => {
  const labels = ROUTE_STRATEGY_IDS.map(strategyLabelKey)
  const descriptions = ROUTE_STRATEGY_IDS.map(strategyDescriptionKey)
  assert.equal(new Set(labels).size, ROUTE_STRATEGY_IDS.length)
  assert.equal(new Set(descriptions).size, ROUTE_STRATEGY_IDS.length)
  assert.equal(strategyLabelKey('nonsense'), 'strategyPriority')
  assert.equal(strategyDescriptionKey('weighted'), 'strategyWeightedDescription')
})

test('only weighted sampling consumes candidate weights', () => {
  assert.equal(strategyUsesWeights('weighted'), true)
  assert.equal(strategyUsesWeights('Weighted'), true)
  for (const id of ROUTE_STRATEGY_IDS.filter((id) => id !== 'weighted')) assert.equal(strategyUsesWeights(id), false)
  assert.equal(strategyUsesWeights(undefined), false)
})

test('weights clamp to the gateway 1–100 range', () => {
  assert.equal(clampWeight(0), 1)
  assert.equal(clampWeight(-5), 1)
  assert.equal(clampWeight(Number.POSITIVE_INFINITY), 1)
  assert.equal(clampWeight(7.4), 7)
  assert.equal(clampWeight(Number.NaN), 1)
  assert.equal(clampWeight(42), 42)
})