/// Upper bound (inclusive) for the chart endpoint's raw-rows branch.
///
/// Semantically this is "1 hour": a `1h` preset on the host detail page
/// should hit the raw 10-second metrics table, not the 5-minute rollup.
/// The literal value is **62 minutes** to absorb the frontend's
/// cache-key rounding in `web/app/lib/api.ts::getMetricsChartRangeUrl`,
/// which `floor`s `start` to a minute boundary and `ceil`s `end` to the
/// next minute boundary. That can inflate the requested window by up
/// to 2 × 60 s − 2 ms ≈ 120 s. A naive 3600 s boundary missed almost
/// every real `1h` request and silently degraded the chart to 5-minute
/// resolution; 3720 s keeps `1h` on raw without bleeding into the next
/// preset (`6h` = 21600 s, far above this).
pub const CHART_RAW_BOUNDARY_SECS: i64 = 62 * 60;

/// Upper bound (inclusive) for serving the **raw** 10-second table from the
/// range endpoint (`fetch_metrics_range`), expressed in *whole hours* and
/// compared against `Duration::num_hours()`.
///
/// `num_hours()` truncates, so a request the frontend inflated to e.g.
/// 6 h 2 m by minute-rounding still resolves to `6` and stays on raw. This
/// is the range-endpoint analogue of the 62-minute buffer baked into
/// [`CHART_RAW_BOUNDARY_SECS`]: do **not** "unify" it into a seconds
/// comparison against `6 * 3600`, that silently degrades the 6 h preset to
/// 5-minute resolution (the exact bug the chart boundary's buffer fixes).
pub(super) const RANGE_RAW_BOUNDARY_HOURS: i64 = 6;

/// Upper bound (inclusive) for serving the 5-minute rollup directly; beyond
/// this both endpoints re-aggregate the rollup into 15-minute buckets. 14 days.
///
/// The two query functions compare this cut in **different units by design** —
/// `fetch_metrics_range` in whole hours ([`ROLLUP_BOUNDARY_HOURS`], to keep the
/// same truncation tolerance as the raw boundary above) and
/// `fetch_chart_metrics_range` in seconds ([`ROLLUP_BOUNDARY_SECS`]). Both name
/// the identical 14-day boundary; keep the two constants in lock-step.
pub(super) const ROLLUP_BOUNDARY_HOURS: i64 = 14 * 24; // 336 h
pub(super) const ROLLUP_BOUNDARY_SECS: i64 = ROLLUP_BOUNDARY_HOURS * 3600;
