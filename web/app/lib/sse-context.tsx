"use client";

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { HostMetricsPayload, HostStatusPayload } from "@/app/types/metrics";
import { issueSseTicket } from "@/app/lib/api";
import { useAuth } from "@/app/auth/AuthContext";
import { clearLiveMetricRows, pushLiveMetricPayload } from "@/app/lib/live-metrics-store";

// ──────────────────────────────────────────
// Context type definitions
// ──────────────────────────────────────────

interface SSEContextValue {
  /** host_key -> latest metrics payload (updated every 10s) */
  metricsMap: Record<string, HostMetricsPayload>;
  /** host_key -> latest status payload (updated on initial connection + on change) */
  statusMap: Record<string, HostStatusPayload>;
  /** EventSource connection state */
  isConnected: boolean;
  /** Pre-computed host list derived from statusMap */
  hostList: HostStatusPayload[];
  /** Number of online hosts */
  onlineCount: number;
  /** Number of offline hosts */
  offlineCount: number;
  /**
   * Purge a host from both live maps after deletion.
   * Without this the statusMap entry lingers forever and a "ghost" row keeps
   * rendering on the Overview until a full page reload.
   */
  removeHost: (hostKey: string) => void;
}

interface SSEHostListValue {
  hostList: HostStatusPayload[];
  onlineCount: number;
  offlineCount: number;
}

const emptyMetricsMap: Record<string, HostMetricsPayload> = {};
const emptyStatusMap: Record<string, HostStatusPayload> = {};
const noopRemoveHost = () => {};

const SSEMetricsContext = createContext<Record<string, HostMetricsPayload>>(emptyMetricsMap);
const SSEStatusContext = createContext<Record<string, HostStatusPayload>>(emptyStatusMap);
const SSEConnectionContext = createContext(false);
const SSEHostListContext = createContext<SSEHostListValue>({
  hostList: [],
  onlineCount: 0,
  offlineCount: 0,
});
const SSERemoveHostContext = createContext<(hostKey: string) => void>(noopRemoveHost);

// ──────────────────────────────────────────
// Reconnection settings
// ──────────────────────────────────────────

// `EventSource` itself defaults its `retry` field to ~3000 ms when the
// server doesn't override it. Matching that floor here means a transient
// hiccup (Wi-Fi handoff, VPN reconnect, brief DNS flap) does not race
// the browser's own reconnect logic; the previous 1000 ms start was
// fast enough to mint a fresh SSE ticket on every micro-flap, which
// trickled into the per-user 2 s ticket cooldown server-side and
// surfaced as 429s during long-running sessions.
const INITIAL_RETRY_MS = 3000;
const MAX_RETRY_MS = 30000;

// ──────────────────────────────────────────
// Provider
// ──────────────────────────────────────────

export function SSEProvider({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  const [metricsMap, setMetricsMap] = useState<
    Record<string, HostMetricsPayload>
  >({});
  const [statusMap, setStatusMap] = useState<
    Record<string, HostStatusPayload>
  >({});
  const [isConnected, setIsConnected] = useState(false);
  const esRef = useRef<EventSource | null>(null);

  // ── Batched SSE update buffers ──
  // Accumulate SSE events in refs, then flush once per animation frame.
  // With 100 hosts this reduces 100+ setState calls per scrape cycle to 1.
  const metricsBufRef = useRef<Record<string, HostMetricsPayload>>({});
  const statusBufRef = useRef<Record<string, HostStatusPayload>>({});
  const offlineKeysBufRef = useRef<Set<string>>(new Set());
  const deletedTombstoneRef = useRef<Map<string, number>>(new Map());
  const rafRef = useRef<number | null>(null);

  const flushBuffers = useCallback(() => {
    rafRef.current = null;

    const metricsBuf = metricsBufRef.current;
    const statusBuf = statusBufRef.current;
    const offlineKeys = offlineKeysBufRef.current;

    const hasMetrics = Object.keys(metricsBuf).length > 0;
    const hasStatus = Object.keys(statusBuf).length > 0;
    const hasOffline = offlineKeys.size > 0;

    if (hasMetrics || hasOffline) {
      setMetricsMap((prev) => {
        // Only remove offline keys that actually exist in the map — avoids
        // creating a new object reference when there's nothing to change.
        const keysToRemove = hasOffline
          ? [...offlineKeys].filter((k) => k in prev)
          : [];
        if (!hasMetrics && keysToRemove.length === 0) return prev;

        const next = hasMetrics ? { ...prev, ...metricsBuf } : { ...prev };
        for (const key of keysToRemove) {
          delete next[key];
        }
        return next;
      });
    }

    if (hasStatus) {
      setStatusMap((prev) => ({ ...prev, ...statusBuf }));
    }

    metricsBufRef.current = {};
    statusBufRef.current = {};
    offlineKeysBufRef.current = new Set();
  }, []);

  const scheduleFlush = useCallback(() => {
    if (rafRef.current === null) {
      rafRef.current = requestAnimationFrame(flushBuffers);
    }
  }, [flushBuffers]);

  const removeHost = useCallback((hostKey: string) => {
    // Drop the host from both user-facing maps and any pending buffered events
    // so a late-arriving SSE frame for the doomed host cannot resurrect it.
    deletedTombstoneRef.current.set(hostKey, Date.now());
    delete metricsBufRef.current[hostKey];
    delete statusBufRef.current[hostKey];
    offlineKeysBufRef.current.delete(hostKey);
    clearLiveMetricRows(hostKey);
    setMetricsMap((prev) => {
      if (!(hostKey in prev)) return prev;
      const next = { ...prev };
      delete next[hostKey];
      return next;
    });
    setStatusMap((prev) => {
      if (!(hostKey in prev)) return prev;
      const next = { ...prev };
      delete next[hostKey];
      return next;
    });
  }, []);

  useEffect(() => {
    // Only connect when authenticated
    if (!user) return;

    let retryMs = INITIAL_RETRY_MS;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let unmounted = false;

    // Ticket-first reconnection: a fresh single-use SSE ticket is issued on
    // every (re)connect. Tickets are atomic/one-shot, so re-using the previous
    // one on EventSource's internal retry would always fail — we must loop
    // through our own handler.
    async function connect() {
      if (unmounted) return;

      // Clean up existing connection if any
      if (esRef.current) {
        esRef.current.close();
        esRef.current = null;
      }

      let ticket: string;
      try {
        const res = await issueSseTicket();
        ticket = res.ticket;
      } catch {
        // `apiCall` already redirects to /login on 401 (stale / rotated JWT).
        // Any other failure — network flap, server restarting — is transient
        // and gets the same exponential backoff as a dropped SSE stream.
        if (unmounted) return;
        setIsConnected(false);
        const delay = retryMs;
        retryMs = Math.min(retryMs * 2, MAX_RETRY_MS);
        retryTimer = setTimeout(() => {
          void connect();
        }, delay);
        return;
      }

      if (unmounted) return;

      const apiBase = process.env.NEXT_PUBLIC_API_URL ?? "";
      // The ticket is opaque, short-lived, and single-use — safe to carry as
      // a query parameter. Never put the long-lived JWT on the URL.
      const url = `${apiBase}/api/stream?key=${encodeURIComponent(ticket)}`;

      const es = new EventSource(url);
      esRef.current = es;

      es.onopen = () => {
        setIsConnected(true);
        retryMs = INITIAL_RETRY_MS; // Reset backoff on successful connection
      };

      es.onerror = () => {
        setIsConnected(false);
        es.close();
        esRef.current = null;

        // Exponential backoff reconnection — each attempt mints a new ticket.
        if (!unmounted) {
          const delay = retryMs;
          retryMs = Math.min(retryMs * 2, MAX_RETRY_MS);
          retryTimer = setTimeout(() => {
            void connect();
          }, delay);
        }
      };

      // event: metrics — dynamic data (CPU, memory, network speed)
      // Buffered: accumulated in ref, flushed once per animation frame
      es.addEventListener("metrics", (e: MessageEvent) => {
        try {
          const payload: HostMetricsPayload = JSON.parse(e.data);
          const tombstonedAt = deletedTombstoneRef.current.get(payload.host_key);
          if (tombstonedAt && Date.now() - tombstonedAt < 5000) {
            return;
          }
          if (tombstonedAt) {
            deletedTombstoneRef.current.delete(payload.host_key);
          }
          pushLiveMetricPayload(payload);
          metricsBufRef.current[payload.host_key] = payload;
          scheduleFlush();
        } catch {
          // Ignore parse errors
        }
      });

      // event: status — static data (Docker, port status)
      // Buffered: accumulated in ref, flushed once per animation frame
      es.addEventListener("status", (e: MessageEvent) => {
        try {
          const payload: HostStatusPayload = JSON.parse(e.data);
          const tombstonedAt = deletedTombstoneRef.current.get(payload.host_key);
          if (tombstonedAt && Date.now() - tombstonedAt < 5000) {
            return;
          }
          if (tombstonedAt) {
            deletedTombstoneRef.current.delete(payload.host_key);
          }
          statusBufRef.current[payload.host_key] = payload;
          if (!payload.is_online) {
            offlineKeysBufRef.current.add(payload.host_key);
          }
          scheduleFlush();
        } catch {
          // Ignore parse errors
        }
      });
    }

    void connect();

    // Must close on component unmount — prevent memory leaks
    return () => {
      unmounted = true;
      if (retryTimer) clearTimeout(retryTimer);
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      if (esRef.current) {
        esRef.current.close();
        esRef.current = null;
      }
    };
    // Reconnect only when the *identity* of the authenticated principal
    // changes — not when AuthContext happens to swap in a fresh `user`
    // object that represents the same person (e.g. after a silent
    // `/api/auth/refresh` rotation, which `setUser` calls with a freshly
    // deserialized payload). The previous `[user, ...]` form treated
    // every refresh as a logout-then-login from this hook's perspective:
    // close the EventSource, mint a new ticket, reconnect — for zero
    // user-observable state change. `user?.id` is the stable identity
    // key; it survives every refresh and only changes on actual
    // login/logout transitions.
    //
    // The closure reads `user` (truthy guard) but we deliberately omit
    // it from deps — only the `id` matters for re-running the effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [user?.id, scheduleFlush]);

  // ── Pre-computed derived state (avoids duplicate O(n) in page + sidebar) ──
  const { hostList, onlineCount, offlineCount } = useMemo(() => {
    const list = Object.values(statusMap);
    let online = 0;
    let offline = 0;
    for (const h of list) {
      if (h.is_online) online++;
      else offline++;
    }
    return { hostList: list, onlineCount: online, offlineCount: offline };
  }, [statusMap]);

  const hostListValue = useMemo(
    () => ({ hostList, onlineCount, offlineCount }),
    [hostList, onlineCount, offlineCount],
  );

  return (
    <SSEConnectionContext.Provider value={isConnected}>
      <SSERemoveHostContext.Provider value={removeHost}>
        <SSEStatusContext.Provider value={statusMap}>
          <SSEHostListContext.Provider value={hostListValue}>
            <SSEMetricsContext.Provider value={metricsMap}>
              {children}
            </SSEMetricsContext.Provider>
          </SSEHostListContext.Provider>
        </SSEStatusContext.Provider>
      </SSERemoveHostContext.Provider>
    </SSEConnectionContext.Provider>
  );
}

// ──────────────────────────────────────────
// Hook
// ──────────────────────────────────────────

/** Custom hook to access SSE data */
export function useSSE(): SSEContextValue {
  const metricsMap = useSSEMetricsMap();
  const statusMap = useSSEStatusMap();
  const isConnected = useSSEConnection();
  const { hostList, onlineCount, offlineCount } = useSSEHostList();
  const removeHost = useRemoveHost();

  return useMemo(
    () => ({
      metricsMap,
      statusMap,
      isConnected,
      hostList,
      onlineCount,
      offlineCount,
      removeHost,
    }),
    [metricsMap, statusMap, isConnected, hostList, onlineCount, offlineCount, removeHost],
  );
}

export function useSSEMetricsMap(): Record<string, HostMetricsPayload> {
  return useContext(SSEMetricsContext);
}

export function useSSEStatusMap(): Record<string, HostStatusPayload> {
  return useContext(SSEStatusContext);
}

export function useSSEConnection(): boolean {
  return useContext(SSEConnectionContext);
}

export function useSSEHostList(): SSEHostListValue {
  return useContext(SSEHostListContext);
}

export function useRemoveHost(): (hostKey: string) => void {
  return useContext(SSERemoveHostContext);
}
