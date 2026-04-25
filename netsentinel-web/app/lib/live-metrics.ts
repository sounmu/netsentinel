import type { HostMetricsPayload, MetricsRow } from "@/app/types/metrics";

export const LIVE_METRICS_BUFFER_MS = 6 * 60 * 1000;
export const LIVE_METRICS_MAX_POINTS = 64;

const REST_LIVE_DEDUPE_MS = 1500;
const SECOND_MS = 1000;

function timestampMs(timestamp: string): number {
  return new Date(timestamp).getTime();
}

function rowTimestampMs(row: Pick<MetricsRow, "timestamp">): number {
  return timestampMs(row.timestamp);
}

function secondBucket(tsMs: number): number {
  return Math.round(tsMs / SECOND_MS);
}

function addToSecondIndex(index: Map<number, number[]>, tsMs: number) {
  const bucket = secondBucket(tsMs);
  const entries = index.get(bucket);
  if (entries) {
    entries.push(tsMs);
  } else {
    index.set(bucket, [tsMs]);
  }
}

function hasNearbyTimestamp(index: Map<number, number[]>, tsMs: number): boolean {
  const bucket = secondBucket(tsMs);
  for (let b = bucket - 1; b <= bucket + 1; b++) {
    const entries = index.get(b);
    if (entries?.some((candidate) => Math.abs(candidate - tsMs) <= REST_LIVE_DEDUPE_MS)) {
      return true;
    }
  }
  return false;
}

export function liveMetricsToRow(liveMetrics: HostMetricsPayload): MetricsRow {
  return {
    id: 0,
    host_key: liveMetrics.host_key,
    display_name: liveMetrics.display_name,
    is_online: liveMetrics.is_online,
    cpu_usage_percent: liveMetrics.cpu_usage_percent,
    memory_usage_percent: liveMetrics.memory_usage_percent,
    load_1min: liveMetrics.load_1min,
    load_5min: liveMetrics.load_5min,
    load_15min: liveMetrics.load_15min,
    networks: {
      total_rx_bytes: liveMetrics.network_rate.total_rx_bytes,
      total_tx_bytes: liveMetrics.network_rate.total_tx_bytes,
      rx_bytes_per_sec: liveMetrics.network_rate.rx_bytes_per_sec,
      tx_bytes_per_sec: liveMetrics.network_rate.tx_bytes_per_sec,
    },
    docker_containers: null,
    ports: null,
    disks: liveMetrics.disks ?? null,
    processes: null,
    temperatures: liveMetrics.temperatures ?? null,
    gpus: null,
    cpu_cores: null,
    network_interfaces: null,
    docker_stats: liveMetrics.docker_stats ?? null,
    timestamp: liveMetrics.timestamp,
  };
}

export function appendLiveMetricRow(
  previousRows: readonly MetricsRow[],
  liveMetrics: HostMetricsPayload,
): readonly MetricsRow[] {
  const row = liveMetricsToRow(liveMetrics);
  const rowTs = rowTimestampMs(row);
  // Invalid timestamp (NaN / Infinity from a corrupted SSE payload) means
  // this row cannot be ordered against others. Return the *same* reference
  // so callers can detect "nothing changed" via `===` and skip notifying
  // subscribers — avoids a spurious `useSyncExternalStore` re-render on
  // every malformed event.
  if (!Number.isFinite(rowTs)) return previousRows;

  const cutoffTs = rowTs - LIVE_METRICS_BUFFER_MS;
  const nextRows: MetricsRow[] = [];

  for (const previous of previousRows) {
    const previousTs = rowTimestampMs(previous);
    if (!Number.isFinite(previousTs)) continue;
    if (previousTs < cutoffTs) continue;
    if (Math.abs(previousTs - rowTs) <= REST_LIVE_DEDUPE_MS) continue;
    nextRows.push(previous);
  }

  nextRows.push(row);
  nextRows.sort((a, b) => rowTimestampMs(a) - rowTimestampMs(b));

  if (nextRows.length > LIVE_METRICS_MAX_POINTS) {
    return nextRows.slice(nextRows.length - LIVE_METRICS_MAX_POINTS);
  }
  return nextRows;
}

export function mergeMetricsRows(
  restRows: readonly MetricsRow[],
  liveRows: readonly MetricsRow[],
  rangeStartMs: number,
  rangeEndMs: number,
): readonly MetricsRow[] {
  if (liveRows.length === 0) return restRows;

  const secondIndex = new Map<number, number[]>();
  for (const row of restRows) {
    const ts = rowTimestampMs(row);
    if (Number.isFinite(ts)) {
      addToSecondIndex(secondIndex, ts);
    }
  }

  const visibleLiveRows: MetricsRow[] = [];
  for (const row of liveRows) {
    const ts = rowTimestampMs(row);
    if (!Number.isFinite(ts)) continue;
    if (ts < rangeStartMs || ts > rangeEndMs) continue;
    if (hasNearbyTimestamp(secondIndex, ts)) continue;

    visibleLiveRows.push(row);
    addToSecondIndex(secondIndex, ts);
  }

  if (visibleLiveRows.length === 0) return restRows;

  return [...restRows, ...visibleLiveRows].sort(
    (a, b) => rowTimestampMs(a) - rowTimestampMs(b),
  );
}
