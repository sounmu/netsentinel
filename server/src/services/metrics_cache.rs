use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::repositories::metrics_repo::{ChartMetricsRow, MetricsRow};
use serde_json::Value;

// ──────────────────────────────────────────────
// TTL cache for long-range metric queries
// ──────────────────────────────────────────────

struct CacheEntry<T> {
    data: Arc<Vec<T>>,
    inserted_at: Instant,
    weight_bytes: usize,
}

pub trait CacheWeight {
    fn cache_weight_bytes(&self) -> usize;
}

/// Internal mutable state held under the cache's single `RwLock`.
///
/// Pulling `entries` and `total_bytes` into one struct guarantees the
/// two stay in lock-step: every code path that mutates the map also has
/// a `&mut` to the byte counter, so a future contributor cannot
/// accidentally update one without the other (the previous design held
/// each in a separate `RwLock` and silently desynced when the second
/// `.write()` returned `None`).
struct CacheInner<T> {
    entries: HashMap<String, CacheEntry<T>>,
    total_bytes: usize,
}

impl<T> CacheInner<T> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            total_bytes: 0,
        }
    }

    fn remove_entry(&mut self, key: &str) -> Option<CacheEntry<T>> {
        let entry = self.entries.remove(key)?;
        self.total_bytes = self.total_bytes.saturating_sub(entry.weight_bytes);
        Some(entry)
    }

    fn prune_expired(&mut self, ttl: Duration) {
        let now = Instant::now();
        let total = &mut self.total_bytes;
        self.entries.retain(|_, entry| {
            let keep = now.duration_since(entry.inserted_at) < ttl;
            if !keep {
                *total = total.saturating_sub(entry.weight_bytes);
            }
            keep
        });
    }

    fn remove_with_prefix(&mut self, prefix: &str) {
        let total = &mut self.total_bytes;
        self.entries.retain(|key, entry| {
            let keep = !key.starts_with(prefix);
            if !keep {
                *total = total.saturating_sub(entry.weight_bytes);
            }
            keep
        });
    }
}

/// Simple in-memory TTL cache for rollup/wide time-range metric queries.
///
/// Prevents repeated DB scans when multiple users view the same dashboard range.
/// Entries expire after `ttl` and are lazily evicted on the next `get` or periodic cleanup.
///
/// Bounded by both `max_entries` and `max_bytes` to cap worst-case memory.
/// v0.3.0 grew the per-sample payload (per-core CPU, per-interface network,
/// per-container docker_stats JSON) 3–5×, so count-only caps can still pin
/// multi-MB Vecs. On insert, expired entries are purged first, then
/// oldest-inserted entries are evicted until both caps fit.
pub struct MetricsQueryCache<T> {
    inner: RwLock<CacheInner<T>>,
    /// Per-key in-flight map for singleflight de-duplication of cache misses.
    ///
    /// Without this, two dashboards opening the same wide-range chart at the
    /// same time both miss the cache and run the SQL twice (or N times for
    /// N concurrent users). On `>14d` queries — which the route comment
    /// explicitly flags as "the 15-min re-aggregation branch" — this used
    /// to dominate SQLite writer contention during dashboard spikes.
    ///
    /// First miss inserts a `Notify` and runs the fetch; concurrent misses
    /// wait on that `Notify` and re-read the cache when notified. The map
    /// is cleared by the leader on completion, bounded by the count of
    /// concurrently-distinct in-flight keys.
    inflight: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Notify>>>,
    ttl: Duration,
    max_entries: usize,
    max_bytes: usize,
}

/// Build a cache key from query parameters.
///
/// Rounds timestamps so near-identical dashboard queries collapse onto a
/// shared cache entry. Keys for ranges ≤ `raw_boundary_secs` use 10 s
/// buckets, ranges within 14 d use 60 s buckets, and wide re-aggregation
/// (> 14 d) uses 300 s buckets. The full-metrics endpoints pass a 6 h
/// raw boundary; the chart endpoint passes 1 h.
pub fn metrics_cache_key(
    host_key: &str,
    start_ts: i64,
    end_ts: i64,
    raw_boundary_secs: i64,
) -> String {
    const ROLLUP_BOUNDARY_SECS: i64 = 14 * 24 * 3600;

    let range = (end_ts - start_ts).max(0);
    let bucket: i64 = if range <= raw_boundary_secs {
        10
    } else if range <= ROLLUP_BOUNDARY_SECS {
        60
    } else {
        300
    };
    let start_rounded = start_ts.div_euclid(bucket) * bucket;
    let end_rounded = (end_ts + bucket - 1).div_euclid(bucket) * bucket;
    format!("{host_key}:{start_rounded}:{end_rounded}")
}

/// Whether a `[start, end]` range should be cached at all. Ranges
/// inside the raw window are excluded because live dashboards already
/// get SWR dedup + SSE live samples and the indexed read is cheap.
pub fn should_cache_metrics_range(start_ts: i64, end_ts: i64, raw_boundary_secs: i64) -> bool {
    (end_ts - start_ts).max(0) > raw_boundary_secs
}

impl<T> MetricsQueryCache<T>
where
    T: CacheWeight,
{
    pub fn new(ttl: Duration, max_entries: usize, max_bytes: usize) -> Self {
        Self {
            inner: RwLock::new(CacheInner::new()),
            inflight: std::sync::Mutex::new(HashMap::new()),
            ttl,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    /// Cache-miss-deduplicated fetch.
    ///
    /// Returns the cached `Arc<Vec<T>>` if present; otherwise either runs
    /// `fetch` itself (the leader) or waits on the leader's notify and
    /// re-reads the cache (the follower). If the leader's fetch errors,
    /// followers fall back to their own `fetch` so a transient SQL error
    /// is not promoted into a sticky cache poison.
    ///
    /// The notify is keyed on the same string as the cache entry so the
    /// caller's `metrics_cache_key(...)` already shapes hits/misses correctly.
    pub async fn get_or_fetch<F, Fut, E>(&self, key: String, fetch: F) -> Result<Arc<Vec<T>>, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<T>, E>>,
    {
        if let Some(cached) = self.get(&key) {
            return Ok(cached);
        }

        let (notify, is_leader) = {
            let mut g = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = g.get(&key).cloned() {
                (existing, false)
            } else {
                let n = Arc::new(tokio::sync::Notify::new());
                g.insert(key.clone(), Arc::clone(&n));
                (n, true)
            }
        };

        if !is_leader {
            let notified = notify.notified();
            tokio::pin!(notified);
            // Register interest *before* re-checking the cache, so a
            // notify_waiters() that lands between this enable() and the
            // .await still wakes us. Without enable(), the future only
            // registers on its first poll inside `.await`, which is
            // exactly the race we are guarding against.
            notified.as_mut().enable();
            if let Some(cached) = self.get(&key) {
                return Ok(cached);
            }
            notified.await;
            if let Some(cached) = self.get(&key) {
                return Ok(cached);
            }
            // Leader errored — fall back to a private fetch.
            let rows = fetch().await?;
            return Ok(Arc::new(rows));
        }

        let result = fetch().await;
        {
            let mut g = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            g.remove(&key);
        }
        match result {
            Ok(rows) => {
                let arc = self.insert(key, rows);
                notify.notify_waiters();
                Ok(arc)
            }
            Err(e) => {
                notify.notify_waiters();
                Err(e)
            }
        }
    }

    /// Get a cached result if it exists and hasn't expired.
    /// Returns an Arc-wrapped Vec for cheap cloning (atomic ref-count increment only).
    pub fn get(&self, key: &str) -> Option<Arc<Vec<T>>> {
        let inner = self.inner.read().ok()?;
        let entry = inner.entries.get(key)?;
        if entry.inserted_at.elapsed() < self.ttl {
            Some(Arc::clone(&entry.data))
        } else {
            None
        }
    }

    /// Insert a query result into the cache and return the Arc-wrapped data.
    /// Avoids the caller needing to clone the Vec before insertion.
    ///
    /// Enforces `max_entries` and `max_bytes` by first draining expired
    /// rows, then evicting oldest-inserted entries until both caps fit.
    /// Oversized single payloads (`weight > max_bytes`) bypass the cache —
    /// they are still returned to the caller, just not retained.
    pub fn insert(&self, key: String, data: Vec<T>) -> Arc<Vec<T>> {
        let weight_bytes = estimate_vec_weight(&data);
        let arc = Arc::new(data);
        if weight_bytes > self.max_bytes {
            // Bumped from `debug` so an unexpectedly large response that
            // silently bypasses the cache surfaces in default ops logs.
            // Frequent emissions here are the operator's signal to raise
            // METRICS_CACHE_MAX_BYTES or narrow the query window.
            tracing::warn!(
                key = %key,
                weight_bytes,
                max_bytes = self.max_bytes,
                "Skipping oversized metrics query cache entry"
            );
            return arc;
        }

        if let Ok(mut inner) = self.inner.write() {
            inner.prune_expired(self.ttl);
            inner.remove_entry(&key);

            while inner.entries.len() >= self.max_entries
                || inner.total_bytes.saturating_add(weight_bytes) > self.max_bytes
            {
                let Some(oldest_key) = inner
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.inserted_at)
                    .map(|(k, _)| k.clone())
                else {
                    break;
                };
                inner.remove_entry(&oldest_key);
            }

            inner.entries.insert(
                key,
                CacheEntry {
                    data: Arc::clone(&arc),
                    inserted_at: Instant::now(),
                    weight_bytes,
                },
            );
            inner.total_bytes = inner.total_bytes.saturating_add(weight_bytes);
        }
        arc
    }

    /// Remove every cached query for a host. Cache keys are
    /// `{host_key}:{rounded_start}:{rounded_end}`, so a prefix match is enough.
    pub fn remove_host(&self, host_key: &str) {
        let prefix = format!("{host_key}:");
        if let Ok(mut inner) = self.inner.write() {
            inner.remove_with_prefix(&prefix);
        }
    }

    /// Current number of entries. Only called from tests today; exposed on
    /// the public API so ops can wire it into `/metrics` later without having
    /// to re-plumb visibility.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.read().map(|i| i.entries.len()).unwrap_or(0)
    }

    #[cfg(test)]
    pub fn total_bytes(&self) -> usize {
        self.inner.read().map(|i| i.total_bytes).unwrap_or(0)
    }

    /// Remove expired entries. Called periodically from a background task.
    pub fn evict_expired(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.prune_expired(self.ttl);
        }
    }
}

fn estimate_vec_weight<T: CacheWeight>(data: &[T]) -> usize {
    std::mem::size_of_val(data)
        + data
            .iter()
            .map(CacheWeight::cache_weight_bytes)
            .sum::<usize>()
}

fn value_weight(value: &Option<Value>) -> usize {
    match value {
        Some(Value::Null) | None => 0,
        Some(inner) => value_weight_inner(inner),
    }
}

fn value_weight_inner(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 8,
        Value::String(s) => s.len(),
        Value::Array(items) => {
            std::mem::size_of_val(items.as_slice())
                + items.iter().map(value_weight_inner).sum::<usize>()
        }
        // serde_json::Map (BTreeMap-like) has ~24 B per node on top of the
        // key + value bytes — negligible per entry but compounds on the
        // hot per-core / per-interface JSON we cache.
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| std::mem::size_of::<(String, Value)>() + k.len() + value_weight_inner(v))
            .sum(),
    }
}

impl CacheWeight for MetricsRow {
    fn cache_weight_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.host_key.len()
            + self.display_name.len()
            + value_weight(&self.networks)
            + value_weight(&self.docker_containers)
            + value_weight(&self.ports)
            + value_weight(&self.disks)
            + value_weight(&self.processes)
            + value_weight(&self.temperatures)
            + value_weight(&self.gpus)
            + value_weight(&self.cpu_cores)
            + value_weight(&self.network_interfaces)
            + value_weight(&self.docker_stats)
    }
}

impl CacheWeight for ChartMetricsRow {
    fn cache_weight_bytes(&self) -> usize {
        // `size_of_val(slice)` on each Vec captures the per-element
        // fixed-size component (e.g. `f32`/`f64` fields, mount counts
        // for ChartDiskInfo). The per-iter `+= s.len()` then layers on
        // the heap-tail of each `String`. Without `size_of_val` the
        // weight tracker undercounted Vec contents by `len * size_of<T>`,
        // which on a 30-day chart with dozens of containers compounded
        // to multiple MB of unaccounted RSS.
        std::mem::size_of::<Self>()
            + self.host_key.len()
            + self.display_name.len()
            + std::mem::size_of_val(self.disks.as_slice())
            + self
                .disks
                .iter()
                .map(|d| d.name.len() + d.mount_point.len())
                .sum::<usize>()
            + std::mem::size_of_val(self.temperatures.as_slice())
            + self
                .temperatures
                .iter()
                .map(|t| t.label.len())
                .sum::<usize>()
            + std::mem::size_of_val(self.docker_stats.as_slice())
            + self
                .docker_stats
                .iter()
                .map(|s| s.container_name.len())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl CacheWeight for usize {
        fn cache_weight_bytes(&self) -> usize {
            *self
        }
    }

    #[test]
    fn metrics_query_cache_enforces_max_entries() {
        // Long TTL so entries never expire — this test exclusively exercises
        // the capacity-based eviction path.
        let cache = MetricsQueryCache::<usize>::new(Duration::from_secs(600), 3, 1024 * 1024);
        for i in 0..10 {
            cache.insert(format!("k{i}"), vec![]);
        }
        assert_eq!(
            cache.len(),
            3,
            "cache must stay at max_entries under flood insert"
        );
        // The most recent three keys are the ones that survive (oldest-first eviction).
        for i in 7..10 {
            assert!(cache.get(&format!("k{i}")).is_some());
        }
        for i in 0..7 {
            assert!(cache.get(&format!("k{i}")).is_none());
        }
    }

    #[test]
    fn make_key_picks_different_bucket_per_tier() {
        // Pin the dynamic-granularity contract: the three tiers
        // (≤ 6 h / ≤ 14 d / > 14 d) emit distinguishable keys for the same
        // absolute start. Without this, the fixed 300 s bucket regression
        // silently resurfaces — live dashboards would see stale data again
        // (see the comment on `metrics_cache_key`).
        let start: i64 = 1_700_000_000;
        let raw_boundary: i64 = 6 * 3600;
        let k_live = metrics_cache_key("h", start, start + 5 * 60, raw_boundary);
        let k_rollup = metrics_cache_key("h", start, start + 12 * 3600, raw_boundary);
        let k_wide = metrics_cache_key("h", start, start + 30 * 86400, raw_boundary);
        assert_ne!(k_live, k_rollup, "live vs rollup must not collide");
        assert_ne!(k_rollup, k_wide, "rollup vs wide must not collide");

        // Live tier advances one bucket after a 10 s shift (frontend's own
        // live rounding granularity in `api.ts`). Pin the boundary so a
        // regression to a coarser server bucket is caught immediately.
        let k_live_next = metrics_cache_key("h", start + 10, start + 10 + 5 * 60, raw_boundary);
        assert_ne!(
            k_live, k_live_next,
            "10 s shift on live range must cross a bucket boundary"
        );
    }

    #[test]
    fn should_cache_range_excludes_raw_window_only() {
        // The full-metrics endpoint refuses to cache anything ≤ 6 h.
        let full_boundary: i64 = 6 * 3600;
        assert!(!should_cache_metrics_range(0, full_boundary, full_boundary));
        assert!(should_cache_metrics_range(
            0,
            full_boundary + 1,
            full_boundary
        ));

        // The chart endpoint passes its own ~1 h boundary (62 min, see
        // `metrics_repo::CHART_RAW_BOUNDARY_SECS`). This test only pins
        // the boundary semantics (`<= boundary` does not cache, `>` does);
        // the literal value mirrors the constant for readability.
        let chart_boundary: i64 = 62 * 60;
        assert!(!should_cache_metrics_range(
            0,
            chart_boundary,
            chart_boundary
        ));
        assert!(should_cache_metrics_range(
            0,
            chart_boundary + 1,
            chart_boundary
        ));
    }

    #[test]
    fn metrics_query_cache_eviction_prefers_expired_over_fresh() {
        // Short TTL; insert two entries, wait past TTL, insert more up to the
        // cap. Expired entries should be purged first, leaving the fresh ones.
        let cache = MetricsQueryCache::<usize>::new(Duration::from_millis(10), 3, 1024 * 1024);
        cache.insert("old1".into(), vec![]);
        cache.insert("old2".into(), vec![]);
        std::thread::sleep(Duration::from_millis(20));
        cache.insert("fresh1".into(), vec![]);
        cache.insert("fresh2".into(), vec![]);
        cache.insert("fresh3".into(), vec![]);
        assert!(cache.get("old1").is_none());
        assert!(cache.get("old2").is_none());
        assert!(cache.get("fresh1").is_some());
        assert!(cache.get("fresh2").is_some());
        assert!(cache.get("fresh3").is_some());
    }

    #[test]
    fn metrics_query_cache_enforces_byte_budget() {
        let cache = MetricsQueryCache::new(Duration::from_secs(600), 10, 256);
        cache.insert("large1".into(), vec![200usize]);
        cache.insert("large2".into(), vec![200usize]);

        assert!(cache.get("large1").is_none());
        assert!(cache.get("large2").is_some());
        assert!(cache.total_bytes() <= 256 + std::mem::size_of::<usize>());
    }

    #[test]
    fn metrics_query_cache_skips_single_entry_over_byte_budget() {
        let cache = MetricsQueryCache::new(Duration::from_secs(600), 10, 256);
        let returned = cache.insert("oversized".into(), vec![300usize]);

        assert_eq!(*returned, vec![300usize]);
        assert!(cache.get("oversized").is_none());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn metrics_query_cache_removes_entries_by_host_key() {
        let cache = MetricsQueryCache::new(Duration::from_secs(600), 10, 1024 * 1024);
        cache.insert("h1:9101:100:200".into(), vec![1usize]);
        cache.insert("h1:9101:200:300".into(), vec![1usize]);
        cache.insert("h2:9101:100:200".into(), vec![1usize]);

        cache.remove_host("h1:9101");

        assert!(cache.get("h1:9101:100:200").is_none());
        assert!(cache.get("h1:9101:200:300").is_none());
        assert!(cache.get("h2:9101:100:200").is_some());
        assert_eq!(cache.len(), 1);
    }
}
