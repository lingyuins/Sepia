//! Built-in route candidate ordering strategies.
//!
//! Every strategy only reorders the candidate list; the shared failover loop
//! still walks the ordered list under the per-request `max_attempts` budget,
//! so a strategy's preferred candidate failing always falls back to the rest.
//! `priority` keeps the saved order (and any plugin `relay.strategy` output).

use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Wire identifiers, in UI display order.
pub const STRATEGY_IDS: [&str; 10] = [
    "priority",
    "weighted",
    "round-robin",
    "random",
    "p2c",
    "least-used",
    "cost-optimized",
    "latency",
    "context-fit",
    "sticky",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Saved order; drain the first candidate before the next.
    Priority,
    /// Weighted random order without replacement (weight per candidate).
    Weighted,
    /// Rotate the saved order by a per-route cursor: 1→2→3→4→1.
    RoundRobin,
    /// Uniform random order without replacement.
    Random,
    /// Pick two random candidates, serve the one with less inflight load.
    P2c,
    /// Lowest inflight upstream load first.
    LeastUsed,
    /// Cheapest configured price per candidate first; unpriced last.
    CostOptimized,
    /// Lowest observed latency (EWMA of time-to-first-byte) first.
    Latency,
    /// Smallest known context window that fits the request first (Model
    /// Plaza metadata); unknown-context candidates next, too-small last.
    ContextFit,
    /// Last known good candidate for this route first.
    Sticky,
}

impl Strategy {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "priority" => Self::Priority,
            "weighted" => Self::Weighted,
            "round-robin" => Self::RoundRobin,
            "random" => Self::Random,
            "p2c" => Self::P2c,
            "least-used" => Self::LeastUsed,
            "cost-optimized" => Self::CostOptimized,
            "latency" => Self::Latency,
            "context-fit" => Self::ContextFit,
            "sticky" => Self::Sticky,
            _ => return None,
        })
    }
}

/// In-memory routing telemetry. Reset on restart by design: strategies fall
/// back to neutral ordering when no samples exist yet.
#[derive(Default)]
pub struct RoutingStats {
    inner: Mutex<StatsInner>,
}

#[derive(Default)]
struct StatsInner {
    /// Concurrent upstream calls per provider.
    inflight: HashMap<i64, u32>,
    /// EWMA of successful time-to-first-byte per (provider, model).
    ewma_ms: HashMap<(i64, String), f64>,
    /// Per-route round-robin cursor.
    cursors: HashMap<i64, usize>,
    /// Last successful (provider, model) per route.
    last_good: HashMap<i64, (i64, String)>,
}

/// Latency sample charged for failed attempts so broken candidates sink
/// below healthy ones without permanently excluding them.
const FAILURE_PENALTY_MS: f64 = 5_000.0;
const EWMA_ALPHA: f64 = 0.3;

impl RoutingStats {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn enter(&self, provider_id: i64) {
        let mut inner = self.inner.lock().unwrap();
        *inner.inflight.entry(provider_id).or_default() += 1;
    }

    pub fn leave(&self, provider_id: i64) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(count) = inner.inflight.get_mut(&provider_id) {
            *count = count.saturating_sub(1);
        }
    }

    /// Records a finished attempt: latency EWMA for every outcome, sticky
    /// last-good only on success.
    pub fn record(
        &self,
        route_id: i64,
        provider_id: i64,
        model: &str,
        success: bool,
        latency_ms: i64,
    ) {
        let mut inner = self.inner.lock().unwrap();
        let sample = if success {
            latency_ms.max(0) as f64
        } else {
            FAILURE_PENALTY_MS
        };
        let entry = inner
            .ewma_ms
            .entry((provider_id, model.to_string()))
            .or_insert(sample);
        *entry = EWMA_ALPHA * sample + (1.0 - EWMA_ALPHA) * *entry;
        if success {
            inner
                .last_good
                .insert(route_id, (provider_id, model.to_string()));
        }
    }

    fn inflight(&self, provider_id: i64) -> u32 {
        self.inner
            .lock()
            .unwrap()
            .inflight
            .get(&provider_id)
            .copied()
            .unwrap_or(0)
    }

    fn ewma(&self, provider_id: i64, model: &str) -> Option<f64> {
        self.inner
            .lock()
            .unwrap()
            .ewma_ms
            .get(&(provider_id, model.to_string()))
            .copied()
    }

    fn next_cursor(&self, route_id: i64, len: usize) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let cursor = inner.cursors.entry(route_id).or_default();
        let current = *cursor % len.max(1);
        *cursor = current + 1;
        current
    }

    fn last_good(&self, route_id: i64) -> Option<(i64, String)> {
        self.inner.lock().unwrap().last_good.get(&route_id).cloned()
    }
}

/// Per-request inputs the ordering needs beyond the candidate list.
pub struct OrderContext<'a> {
    pub route_id: i64,
    /// Configured channel prices (micros per token), only priced entries.
    pub prices: &'a HashMap<(i64, String), (i64, i64)>,
    /// Known context windows from synced Model Plaza metadata.
    pub contexts: &'a HashMap<String, i64>,
    /// Rough input-token estimate of the inbound request.
    pub estimated_tokens: i64,
}

/// Reorders `candidates` per the route's strategy. Unknown strategy strings
/// behave as `priority` (saved order). Weights are parallel to candidates.
pub fn order(
    strategy: &str,
    route_id: i64,
    candidates: &[(i64, String)],
    weights: &[i64],
    stats: &RoutingStats,
    context: &OrderContext<'_>,
) -> Vec<(i64, String)> {
    let Some(strategy) = Strategy::parse(strategy) else {
        return candidates.to_vec();
    };
    order_with(
        strategy,
        route_id,
        candidates,
        weights,
        stats,
        context,
        &mut rand::rng(),
    )
}

fn order_with(
    strategy: Strategy,
    route_id: i64,
    candidates: &[(i64, String)],
    weights: &[i64],
    stats: &RoutingStats,
    context: &OrderContext<'_>,
    rng: &mut impl Rng,
) -> Vec<(i64, String)> {
    if candidates.len() <= 1 || strategy == Strategy::Priority {
        return candidates.to_vec();
    }
    let mut ordered = candidates.to_vec();
    match strategy {
        Strategy::Priority => {}
        Strategy::Weighted => {
            // Weighted sampling without replacement: repeatedly draw one
            // candidate proportional to its weight, then remove it.
            let mut pool: Vec<(usize, i64)> = candidates
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    (
                        index,
                        weights.get(index).copied().unwrap_or(1).clamp(1, 100),
                    )
                })
                .collect();
            ordered.clear();
            while !pool.is_empty() {
                let total: i64 = pool.iter().map(|(_, weight)| weight).sum();
                let mut draw = rng.random_range(0..total);
                let mut picked = pool.len() - 1;
                for (position, (_, weight)) in pool.iter().enumerate() {
                    if draw < *weight {
                        picked = position;
                        break;
                    }
                    draw -= weight;
                }
                let (index, _) = pool.remove(picked);
                ordered.push(candidates[index].clone());
            }
        }
        Strategy::RoundRobin => {
            let start = stats.next_cursor(route_id, candidates.len());
            ordered.rotate_left(start);
        }
        Strategy::Random => {
            for index in (1..ordered.len()).rev() {
                ordered.swap(index, rng.random_range(0..=index));
            }
        }
        Strategy::P2c => {
            let first = rng.random_range(0..ordered.len());
            let second = rng.random_range(0..ordered.len() - 1);
            let second = if second >= first { second + 1 } else { second };
            let lighter =
                if stats.inflight(candidates[first].0) <= stats.inflight(candidates[second].0) {
                    first
                } else {
                    second
                };
            ordered.swap(0, lighter);
        }
        Strategy::LeastUsed => {
            ordered.sort_by_key(|(provider_id, _)| stats.inflight(*provider_id));
        }
        Strategy::CostOptimized => {
            ordered.sort_by_key(|candidate| {
                context
                    .prices
                    .get(candidate)
                    .map(|(input, output)| input.saturating_add(*output))
                    .unwrap_or(i64::MAX)
            });
        }
        Strategy::Latency => {
            ordered.sort_by(|a, b| {
                stats
                    .ewma(a.0, &a.1)
                    .unwrap_or(0.0)
                    .total_cmp(&stats.ewma(b.0, &b.1).unwrap_or(0.0))
            });
        }
        Strategy::ContextFit => {
            // 0 = known to fit (smallest window wins), 1 = unknown, 2 = too small.
            let bucket = |model: &str| match context.contexts.get(model) {
                Some(window) if *window >= context.estimated_tokens => (0, *window),
                Some(_) => (2, 0),
                None => (1, 0),
            };
            ordered.sort_by_key(|(_, model)| bucket(model));
        }
        Strategy::Sticky => {
            if let Some(last) = stats.last_good(route_id)
                && let Some(position) = ordered.iter().position(|candidate| *candidate == last)
            {
                ordered.swap(0, position);
            }
        }
    }
    ordered
}

/// Rough inbound token estimate: ~4 characters per token across system
/// prompt and message text. Display/selection heuristic only — real usage
/// still comes from upstream responses.
pub fn estimate_tokens(system: Option<&str>, messages: &[crate::protocol::Message]) -> i64 {
    let chars: usize = system.map(str::len).unwrap_or(0)
        + messages
            .iter()
            .map(|message| message.text.len())
            .sum::<usize>();
    ((chars as i64 + 3) / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn candidates() -> Vec<(i64, String)> {
        vec![
            (1, "a".into()),
            (2, "b".into()),
            (3, "c".into()),
            (4, "d".into()),
        ]
    }

    fn context() -> OrderContext<'static> {
        static PRICES: std::sync::LazyLock<HashMap<(i64, String), (i64, i64)>> =
            std::sync::LazyLock::new(HashMap::new);
        static CONTEXTS: std::sync::LazyLock<HashMap<String, i64>> =
            std::sync::LazyLock::new(HashMap::new);
        OrderContext {
            route_id: 7,
            prices: &PRICES,
            contexts: &CONTEXTS,
            estimated_tokens: 100,
        }
    }

    fn run(
        strategy: Strategy,
        stats: &RoutingStats,
        context: &OrderContext<'_>,
        seed: u64,
    ) -> Vec<(i64, String)> {
        order_with(
            strategy,
            7,
            &candidates(),
            &[1, 1, 1, 1],
            stats,
            context,
            &mut StdRng::seed_from_u64(seed),
        )
    }

    #[test]
    fn parse_accepts_all_ids_and_rejects_unknown() {
        for id in STRATEGY_IDS {
            assert!(Strategy::parse(id).is_some(), "{id}");
        }
        assert!(Strategy::parse("failover").is_none());
        assert!(Strategy::parse("").is_none());
    }

    #[test]
    fn priority_and_unknown_keep_saved_order() {
        let stats = RoutingStats::default();
        assert_eq!(run(Strategy::Priority, &stats, &context(), 1), candidates());
        assert_eq!(
            order("nonsense", 7, &candidates(), &[], &stats, &context()),
            candidates()
        );
    }

    #[test]
    fn round_robin_cycles_through_all_candidates() {
        let stats = RoutingStats::default();
        for expected_start in 0..4 {
            let ordered = run(Strategy::RoundRobin, &stats, &context(), 0);
            assert_eq!(ordered[0], candidates()[expected_start]);
            assert_eq!(ordered.len(), 4);
        }
        // Fifth request wraps to the first candidate again.
        assert_eq!(
            run(Strategy::RoundRobin, &stats, &context(), 0)[0],
            candidates()[0]
        );
    }

    #[test]
    fn random_and_weighted_are_permutations() {
        let stats = RoutingStats::default();
        for strategy in [Strategy::Random, Strategy::Weighted, Strategy::P2c] {
            for seed in 0..8 {
                let mut ordered = run(strategy, &stats, &context(), seed);
                ordered.sort();
                assert_eq!(ordered, candidates());
            }
        }
    }

    #[test]
    fn weighted_respects_weight_skew() {
        let stats = RoutingStats::default();
        let mut first_counts = [0usize; 4];
        for seed in 0..400 {
            let ordered = order_with(
                Strategy::Weighted,
                7,
                &candidates(),
                &[90, 5, 3, 2],
                &stats,
                &context(),
                &mut StdRng::seed_from_u64(seed),
            );
            let position = candidates().iter().position(|c| *c == ordered[0]).unwrap();
            first_counts[position] += 1;
        }
        assert!(first_counts[0] > 300, "{first_counts:?}");
        assert!(first_counts[1] < 60, "{first_counts:?}");
    }

    #[test]
    fn least_used_and_p2c_prefer_idle_providers() {
        let stats = RoutingStats::default();
        stats.enter(1);
        stats.enter(1);
        stats.enter(3);
        let ordered = run(Strategy::LeastUsed, &stats, &context(), 0);
        assert_eq!(ordered[0].0, 2);
        assert_eq!(ordered.last().unwrap().0, 1);
        // With provider 1 loaded, p2c must never promote it over an idle pick.
        for seed in 0..16 {
            let ordered = run(Strategy::P2c, &stats, &context(), seed);
            assert_ne!(ordered[0].0, 1);
        }
        stats.leave(1);
        stats.leave(1);
        assert_eq!(stats.inflight(1), 0);
    }

    #[test]
    fn cost_optimized_sorts_priced_first_then_unpriced() {
        let stats = RoutingStats::default();
        let mut prices = HashMap::new();
        prices.insert((3, "c".to_string()), (100, 200));
        prices.insert((1, "a".to_string()), (500, 500));
        let context = OrderContext {
            prices: &prices,
            ..context()
        };
        let ordered = run(Strategy::CostOptimized, &stats, &context, 0);
        assert_eq!(ordered[0].0, 3);
        assert_eq!(ordered[1].0, 1);
        // Unpriced candidates keep relative saved order behind priced ones.
        assert_eq!(ordered[2].0, 2);
        assert_eq!(ordered[3].0, 4);
    }

    #[test]
    fn latency_prefers_observed_fast_and_penalizes_failures() {
        let stats = RoutingStats::default();
        stats.record(7, 2, "b", true, 40);
        stats.record(7, 3, "c", false, 0);
        let ordered = run(Strategy::Latency, &stats, &context(), 0);
        // Unknown candidates (no samples) rank ahead of measured ones.
        assert_eq!(ordered[0].0, 1);
        assert_eq!(ordered[2].0, 2);
        assert_eq!(ordered[3].0, 3);
        // Sticky remembers only the successful candidate.
        let sticky = run(Strategy::Sticky, &stats, &context(), 0);
        assert_eq!(sticky[0], (2, "b".to_string()));
    }

    #[test]
    fn context_fit_prefers_smallest_fitting_window() {
        let stats = RoutingStats::default();
        let mut contexts_map = HashMap::new();
        contexts_map.insert("a".to_string(), 200_000);
        contexts_map.insert("b".to_string(), 8_000);
        contexts_map.insert("c".to_string(), 50);
        let context = OrderContext {
            contexts: &contexts_map,
            estimated_tokens: 100,
            ..context()
        };
        let ordered = run(Strategy::ContextFit, &stats, &context, 0);
        assert_eq!(ordered[0].1, "b"); // smallest fitting
        assert_eq!(ordered[1].1, "a"); // larger fitting
        assert_eq!(ordered[2].1, "d"); // unknown
        assert_eq!(ordered[3].1, "c"); // too small
    }

    #[test]
    fn single_candidate_is_untouched() {
        let stats = RoutingStats::default();
        for id in STRATEGY_IDS {
            let one = vec![(9, "solo".to_string())];
            assert_eq!(order(id, 7, &one, &[5], &stats, &context()), one);
        }
    }

    #[test]
    fn estimate_tokens_counts_system_and_messages() {
        use crate::protocol::Message;
        let message = Message {
            role: crate::protocol::Role::User,
            text: "0123456789ab".into(),
            content: vec![],
        };
        assert_eq!(estimate_tokens(Some("abcd"), &[message]), 4);
        assert_eq!(estimate_tokens(None, &[]), 1);
    }
}
