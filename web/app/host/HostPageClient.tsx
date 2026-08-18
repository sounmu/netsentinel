"use client";

import dynamic from "next/dynamic";
import { notFound, useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";
import useSWR from "swr";
import {
  useSSEConnection,
  useSSEMetricsMap,
  useSSEStatusMap,
} from "@/app/lib/sse-context";
import { fetcher, getHostsUrl } from "@/app/lib/api";
import DockerGrid from "@/app/components/DockerGrid";
const TimeSeriesChart = dynamic(
  () => import("@/app/components/TimeSeriesChart"),
  { ssr: false, loading: () => <div className="skeleton" style={{ height: 300 }} /> },
);
import PortList from "@/app/components/PortList";
import GpuCard from "@/app/components/GpuCard";
import UptimeHistory from "@/app/components/UptimeHistory";
import { getHostStatus } from "@/app/lib/status";
import {
  Panel, PanelHeader, EmptyState, SkeletonRows, StatusDot, Button,
} from "@/app/components/ui";
import {
  Activity,
  ArrowLeft,
  Wifi,
  Monitor,
  Clock,
  Globe,
  Cpu,
  MemoryStick,
} from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";

/** Maps host status onto the shared dot tones. */
const DOT_TONE = { online: "ok", pending: "warn", offline: "off" } as const;

/** Format uptime from boot_time (Unix timestamp seconds).
 *  <24h → "Xh Xm", ≥24h → "Xd Xh"
 *
 *  Pure function — `now` is supplied by the caller so the same render input
 *  always yields the same output. React 19's compiler is allowed to memoize
 *  components that call this; reading `Date.now()` inside would silently
 *  freeze the displayed uptime once the component is cached. The caller
 *  drives ticks via `useNowSeconds` below. */
function formatUptime(bootTime: number, nowSecs: number): string {
  const secs = Math.max(nowSecs - bootTime, 0);
  const minutes = Math.floor(secs / 60) % 60;
  const hours = Math.floor(secs / 3600) % 24;
  const days = Math.floor(secs / 86400);
  if (days > 0) return `${days}d ${hours}h`;
  return `${hours}h ${minutes}m`;
}

/** A re-rendering "now" in unix-seconds, ticking every minute. Anything
 *  finer than that is wasted re-renders since `formatUptime` rounds to
 *  minutes anyway. Initial value is taken from `Date.now()` lazily so the
 *  initial render still produces accurate output without waiting for the
 *  first interval tick. */
function useNowSeconds(): number {
  const [now, setNow] = useState<number>(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const id = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 60_000);
    return () => clearInterval(id);
  }, []);
  return now;
}

/** A titled section on the host detail page. Thin wrapper over the shared
 *  Panel so every section header on this page matches the rest of the app. */
function SectionCard({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <Panel>
      <PanelHeader title={title} />
      <div className="section-body">{children}</div>
    </Panel>
  );
}

export default function HostPageClient() {
  const searchParams = useSearchParams();
  const decodedHostKey = searchParams.get("key") ?? "";
  const router = useRouter();

  const metricsMap = useSSEMetricsMap();
  const statusMap = useSSEStatusMap();
  const isConnected = useSSEConnection();
  const { t } = useI18n();
  const nowSecs = useNowSeconds();

  // Deterministic "does this host exist?" probe. We don't trust the SSE
  // race — `isConnected` flips true the instant the EventSource opens
  // and the per-host `status` event arrives on the next tick, so a
  // page refresh landing in that gap used to call `notFound()` before
  // the initial snapshot hit `statusMap`. Pending hosts that never
  // emit a `status` event at all made the race permanent. A single
  // SWR fetch against `/api/hosts` gives us a definitive answer:
  // either the key is in the list (valid page) or it isn't (real 404).
  //
  // Refresh on a 60 s cadence + on tab focus so an admin who deletes/adds
  // a host in another tab does not leave this page stuck on a stale
  // notFound() decision indefinitely.
  const { data: hostsList, error: hostsError } = useSWR<Array<{ host_key: string }>>(
    getHostsUrl(),
    fetcher,
    { refreshInterval: 60_000, revalidateOnFocus: true }
  );

  const liveMetrics = metricsMap[decodedHostKey] ?? null;
  const statusData = statusMap[decodedHostKey] ?? null;

  const displayName = liveMetrics?.display_name ?? statusData?.display_name ?? decodedHostKey;
  const hasData = liveMetrics !== null || statusData !== null;

  const ports = statusData?.ports ?? [];
  const gpus = statusData?.gpus ?? [];
  const dockerContainers = statusData?.docker_containers ?? [];
  const latestTimestamp = liveMetrics?.timestamp ?? statusData?.last_seen ?? null;
  const hasDockerData = dockerContainers.length > 0;

  const isOnline = liveMetrics?.is_online ?? statusData?.is_online;
  const hostStatus = latestTimestamp
    ? getHostStatus(latestTimestamp, isOnline, statusData?.scrape_interval_secs)
    : "pending";

  // Fire `notFound()` only after `/api/hosts` has definitively answered.
  // While it's pending (hostsList === undefined && no error) we render
  // the normal loading / skeleton path.
  if (
    decodedHostKey &&
    hostsList &&
    !hostsList.some((h) => h.host_key === decodedHostKey)
  ) {
    notFound();
  }
  // If the hosts endpoint itself failed, fall back to the old
  // SSE-based guard so the page still shows "not found" for truly
  // bogus keys on degraded networks.
  if (
    decodedHostKey &&
    hostsError &&
    isConnected &&
    !hasData &&
    !(decodedHostKey in statusMap)
  ) {
    notFound();
  }

  return (
    <div className="page-content fade-in">
      {/* Identity header — back, name, live status, and the machine facts
          that stay true regardless of which chart you are looking at. */}
      <header className="host-header">
        <div className="host-header__row">
          <Button
            variant="ghost"
            size="sm"
            icon
            onClick={() => router.push("/")}
            aria-label={t.host.backToOverview}
          >
            <ArrowLeft size={15} aria-hidden="true" />
          </Button>
          <StatusDot tone={DOT_TONE[hostStatus]} />
          <h1 className="host-header__name">{displayName}</h1>
        </div>

        <div className="host-facts">
          {statusData?.ip_address && (
            <span className="host-fact">
              <Globe size={13} aria-hidden="true" />
              <span className="mono">{statusData.ip_address}</span>
            </span>
          )}
          {statusData?.boot_time && (
            <span className="host-fact">
              <Clock size={13} aria-hidden="true" />
              {t.host.uptime} {formatUptime(statusData.boot_time, nowSecs)}
            </span>
          )}
          {statusData?.os_info && (
            <span className="host-fact">
              <Monitor size={13} aria-hidden="true" />
              {statusData.os_info}
            </span>
          )}
          {statusData?.cpu_model && (
            <span className="host-fact">
              <Cpu size={13} aria-hidden="true" />
              {statusData.cpu_model}
            </span>
          )}
          {statusData?.memory_total_mb != null && (
            <span className="host-fact">
              <MemoryStick size={13} aria-hidden="true" />
              <span className="mono">
                {statusData.memory_total_mb >= 1024
                  ? `${(statusData.memory_total_mb / 1024).toFixed(1)} GB`
                  : `${statusData.memory_total_mb} MB`}
              </span>
            </span>
          )}

          {/* Fallback while the agent has not reported system info yet. */}
          {!statusData?.ip_address && !statusData?.os_info && (
            <>
              {displayName !== decodedHostKey && (
                <span className="host-fact">
                  <Globe size={13} aria-hidden="true" />
                  <span className="mono">{decodedHostKey}</span>
                </span>
              )}
              {latestTimestamp && (
                <span className="host-fact">
                  <Clock size={13} aria-hidden="true" />
                  <span className="mono">
                    {new Date(latestTimestamp).toLocaleString()}
                  </span>
                </span>
              )}
              {liveMetrics && (
                <>
                  <span className="host-fact">
                    <Cpu size={13} aria-hidden="true" />
                    <span className="mono">
                      CPU {liveMetrics.cpu_usage_percent.toFixed(1)}%
                    </span>
                  </span>
                  <span className="host-fact">
                    <Activity size={13} aria-hidden="true" />
                    <span className="mono">
                      RAM {liveMetrics.memory_usage_percent.toFixed(1)}%
                    </span>
                  </span>
                </>
              )}
            </>
          )}
        </div>
      </header>

      {!isConnected && !hasData && <SkeletonRows count={3} height={200} />}

      {isConnected && !hasData && (
        <Panel>
          <EmptyState
            icon={<Wifi size={28} aria-hidden="true" />}
            title={t.host.noMetrics}
            description={t.host.noMetricsHint}
          />
        </Panel>
      )}

      {hasData && (
        <>
          <TimeSeriesChart hostKey={decodedHostKey} />

          {/* Daily uptime breakdown — day boundaries are in the workspace
              timezone reported by the API, labelled accordingly. */}
          <div className="host-detail-half-grid">
            <SectionCard title={t.host.uptimeHistory}>
              <UptimeHistory hostKey={decodedHostKey} />
            </SectionCard>
            {ports.length > 0 && (
              <SectionCard title={`${t.host.portStatus} (${ports.length})`}>
                <PortList ports={ports} />
              </SectionCard>
            )}
          </div>

          {hasDockerData && (
            <SectionCard title={`${t.host.dockerContainers} (${dockerContainers.length})`}>
              <DockerGrid containers={dockerContainers} />
            </SectionCard>
          )}

          {gpus.length > 0 && (
            <div className="host-detail-half-grid">
              <SectionCard title={`${t.host.gpu} (${gpus.length})`}>
                <GpuCard gpus={gpus} />
              </SectionCard>
            </div>
          )}
        </>
      )}
    </div>
  );
}
