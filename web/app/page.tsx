"use client";

import Link from "next/link";
import {
  useSSEConnection,
  useSSEMetricsMap,
  useSSEStatusMap,
} from "@/app/lib/sse-context";
import { getHostStatus, HostStatus } from "@/app/lib/status";
import React, { useMemo } from "react";
import { useI18n } from "@/app/i18n/I18nContext";
import { Activity, LayoutDashboard } from "lucide-react";
import { formatNetworkSpeed } from "@/app/lib/formatters";
import { PageHeader } from "@/app/components/PageHeader";
import { Meter, Panel, EmptyState, SkeletonRows, StatusDot } from "@/app/components/ui";

/** Maps host status onto the shared dot tones. */
const DOT_TONE = { online: "ok", pending: "warn", offline: "off" } as const;

interface HostRow {
  host_key: string;
  display_name: string;
  is_online: boolean;
  last_seen: string | null;
  status: HostStatus;
  cpu: number;
  ram: number;
  disk: number; // root disk usage %
  load: number;
  networkRx: number;
  networkTx: number;
}

export default function HomePage() {
  const metricsMap = useSSEMetricsMap();
  const statusMap = useSSEStatusMap();
  const isConnected = useSSEConnection();
  const { t } = useI18n();

  const { hosts, onlineCount, offlineCount } = useMemo(() => {
    const list: HostRow[] = Object.values(statusMap).map((status) => {
      const metrics = metricsMap[status.host_key];
      const lastSeen = metrics?.timestamp ?? status.last_seen ?? null;
      const isOnline = metrics?.is_online ?? status.is_online ?? false;
      const hostStatus = getHostStatus(lastSeen, isOnline, status.scrape_interval_secs);

      // Root disk usage: pick "/" mount or highest usage partition
      const disks = status.disks ?? [];
      let diskPct = 0;
      if (disks.length > 0) {
        const root = disks.find((d) => d.mount_point === "/");
        diskPct = root ? root.usage_percent : Math.max(...disks.map((d) => d.usage_percent));
      }

      return {
        host_key: status.host_key,
        display_name: metrics?.display_name ?? status.display_name,
        is_online: isOnline,
        last_seen: lastSeen,
        status: hostStatus,
        cpu: metrics?.cpu_usage_percent ?? 0,
        ram: metrics?.memory_usage_percent ?? 0,
        disk: diskPct,
        load: metrics?.load_1min ?? 0,
        networkRx: metrics?.network_rate?.rx_bytes_per_sec ?? 0,
        networkTx: metrics?.network_rate?.tx_bytes_per_sec ?? 0,
      };
    });

    list.sort((a, b) => {
      const order: Record<HostStatus, number> = { online: 0, pending: 1, offline: 2 };
      const diff = order[a.status] - order[b.status];
      if (diff !== 0) return diff;
      return a.display_name.localeCompare(b.display_name);
    });

    let online = 0;
    let offline = 0;
    for (const h of list) {
      if (h.status === "online") online++;
      else if (h.status === "offline") offline++;
    }
    return { hosts: list, onlineCount: online, offlineCount: offline };
  }, [statusMap, metricsMap]);

  const isLoading = !isConnected && hosts.length === 0;

  return (
    <div className="page-content fade-in">
      <PageHeader
        icon={<LayoutDashboard size={16} aria-hidden="true" />}
        title={t.overview.title}
        badge={hosts.length}
        description={t.overview.description}
        right={
          (onlineCount > 0 || offlineCount > 0) ? (
            <div className="page-header__stats">
              {onlineCount > 0 && (
                <span className="page-header__stats-item">
                  <StatusDot tone="ok" />
                  <b>{onlineCount}</b> {t.overview.online}
                </span>
              )}
              {offlineCount > 0 && (
                <span className="page-header__stats-item">
                  <StatusDot tone="crit" />
                  <b>{offlineCount}</b> {t.overview.offline}
                </span>
              )}
            </div>
          ) : undefined
        }
      />

      <Panel>
        {isLoading && <SkeletonRows count={4} />}

        {!isLoading && hosts.length === 0 && (
          <EmptyState
            icon={<Activity size={28} aria-hidden="true" />}
            title={t.overview.noAgents}
            description={t.overview.noAgentsHint}
          />
        )}

        {!isLoading && hosts.length > 0 && (
          <div className="systems-table-wrap">
            <table className="systems-table">
              <thead>
                <tr>
                  <th style={{ width: "26%" }}>{t.overview.tableHeaders.system}</th>
                  <th style={{ width: "14%" }}>{t.overview.tableHeaders.cpu}</th>
                  <th style={{ width: "14%" }}>{t.overview.tableHeaders.memory}</th>
                  <th style={{ width: "14%" }}>{t.overview.tableHeaders.disk}</th>
                  <th style={{ width: "9%" }}>{t.overview.tableHeaders.load}</th>
                  <th style={{ width: "11%" }}>{t.overview.tableHeaders.netRx}</th>
                  <th style={{ width: "11%" }}>{t.overview.tableHeaders.netTx}</th>
                </tr>
              </thead>
              <tbody>
                {hosts.map((host) => {
                  const offline = host.status !== "online";
                  const dash = <span className="host-cell__dash">—</span>;
                  return (
                    <tr key={host.host_key}>
                      <td>
                        {/* The link wraps the whole identity cell. The table
                            previously set `cursor: pointer` on the entire row
                            while only this text was clickable, so most of the
                            row looked interactive but did nothing. */}
                        <Link
                          href={`/host/?key=${encodeURIComponent(host.host_key)}`}
                          // `prefetch={false}` because Next.js fetches the route
                          // chunk by walking `/host/?key=…` in dev/preview mode,
                          // which our `output: 'export'` + `ServeDir` setup
                          // resolves to a 404 (the static asset lives at
                          // `/host/index.html`, query string is irrelevant to
                          // ServeDir). Disabling prefetch keeps the navigation
                          // path identical (Next still hydrates the cached
                          // chunk on click) without the noisy 404s in server
                          // logs and DevTools Network panel.
                          prefetch={false}
                          className="row-link host-cell"
                        >
                          <StatusDot tone={DOT_TONE[host.status]} />
                          <span className="host-cell__id">
                            <span className="host-cell__name">{host.display_name}</span>
                            {host.display_name !== host.host_key && (
                              <span className="host-cell__key">{host.host_key}</span>
                            )}
                          </span>
                        </Link>
                      </td>
                      <td>{offline ? dash : <Meter value={host.cpu} />}</td>
                      <td>{offline ? dash : <Meter value={host.ram} />}</td>
                      <td>{offline ? dash : <Meter value={host.disk} />}</td>
                      <td className="num">{offline ? dash : host.load.toFixed(2)}</td>
                      <td className="num">
                        {offline ? dash : formatNetworkSpeed(host.networkRx)}
                      </td>
                      <td className="num">
                        {offline ? dash : formatNetworkSpeed(host.networkTx)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
