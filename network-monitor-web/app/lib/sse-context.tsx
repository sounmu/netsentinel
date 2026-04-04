"use client";

import React, {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { HostMetricsPayload, HostStatusPayload } from "@/app/types/metrics";
import { getUserToken } from "@/app/lib/api";
import { useAuth } from "@/app/auth/AuthContext";

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
}

const SSEContext = createContext<SSEContextValue>({
  metricsMap: {},
  statusMap: {},
  isConnected: false,
});

// ──────────────────────────────────────────
// Reconnection settings
// ──────────────────────────────────────────

const INITIAL_RETRY_MS = 1000;
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

  useEffect(() => {
    // Only connect when authenticated
    if (!user) return;

    let retryMs = INITIAL_RETRY_MS;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let unmounted = false;

    function connect() {
      if (unmounted) return;

      // Clean up existing connection if any
      if (esRef.current) {
        esRef.current.close();
        esRef.current = null;
      }

      const apiBase =
        process.env.NEXT_PUBLIC_API_URL ?? "http://127.0.0.1:3000";
      const token = getUserToken();
      const params = token ? `?key=${encodeURIComponent(token)}` : "";
      const url = `${apiBase}/api/stream${params}`;

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

        // Exponential backoff reconnection
        if (!unmounted) {
          const delay = retryMs;
          retryMs = Math.min(retryMs * 2, MAX_RETRY_MS);
          retryTimer = setTimeout(connect, delay);
        }
      };

      // event: metrics — dynamic data (CPU, memory, network speed)
      // Use host_key as map key to prevent display_name collisions
      es.addEventListener("metrics", (e: MessageEvent) => {
        try {
          const payload: HostMetricsPayload = JSON.parse(e.data);
          setMetricsMap((prev) => ({ ...prev, [payload.host_key]: payload }));
        } catch {
          // Ignore parse errors (defense against server bugs)
        }
      });

      // event: status — static data (Docker, port status)
      // When an offline event (is_online: false) is received, remove the key from metricsMap
      // to prevent stale data residue (last CPU/RAM values lingering for offline hosts)
      es.addEventListener("status", (e: MessageEvent) => {
        try {
          const payload: HostStatusPayload = JSON.parse(e.data);
          setStatusMap((prev) => ({ ...prev, [payload.host_key]: payload }));

          // Remove stale metrics data on offline transition
          if (!payload.is_online) {
            setMetricsMap((prev) => {
              const next = { ...prev };
              delete next[payload.host_key];
              return next;
            });
          }
        } catch {
          // Ignore parse errors
        }
      });
    }

    connect();

    // Must close on component unmount — prevent memory leaks
    return () => {
      unmounted = true;
      if (retryTimer) clearTimeout(retryTimer);
      if (esRef.current) {
        esRef.current.close();
        esRef.current = null;
      }
    };
  }, [user]); // Reconnect when auth state changes

  return (
    <SSEContext.Provider value={{ metricsMap, statusMap, isConnected }}>
      {children}
    </SSEContext.Provider>
  );
}

// ──────────────────────────────────────────
// Hook
// ──────────────────────────────────────────

/** Custom hook to access SSE data */
export function useSSE(): SSEContextValue {
  return useContext(SSEContext);
}
