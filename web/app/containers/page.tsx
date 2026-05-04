"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { Activity, ArrowDown, ArrowUp, ArrowUpDown, Box, Server } from "lucide-react";
import { PageHeader } from "@/app/components/PageHeader";
import { useI18n } from "@/app/i18n/I18nContext";
import { useSSE } from "@/app/lib/sse-context";
import { formatBytes } from "@/app/lib/formatters";
import {
  getHostStatus,
  STATUS_DOT_CLASS,
  type HostStatus,
} from "@/app/lib/status";
import type { DockerContainer, DockerContainerStats } from "@/app/types/metrics";

function meterTone(pct: number): string {
  if (pct >= 85) return "var(--md-sys-color-error)";
  if (pct >= 60) return "var(--md-custom-color-warning)";
  return "var(--md-custom-color-success)";
}

function InlineMeter({ value }: { value: number; label?: string }) {
  const pct = Math.min(Math.max(value, 0), 100);
  const tone = meterTone(pct);
  return (
    <div className="flex items-center gap-3">
      <span
        className="min-w-[44px] shrink-0 text-right tabular-nums text-[var(--md-sys-color-on-surface)]"
        style={{ font: "var(--md-sys-typescale-label-medium)" }}
      >
        {pct.toFixed(1)}%
      </span>
      <span className="h-[4px] min-w-[48px] flex-1 overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]">
        <span
          className="block h-full rounded-full"
          style={{
            width: `${pct}%`,
            background: tone,
            transition:
              "width var(--md-sys-motion-duration-medium2) var(--md-sys-motion-easing-standard), background var(--md-sys-motion-duration-medium2) var(--md-sys-motion-easing-standard)",
          }}
        />
      </span>
    </div>
  );
}

type ContainerHealth = "attention" | "running" | "stopped";
type SortKey = "container" | "host" | "status" | "cpu" | "memory" | "network" | "storage" | "image";
type SortDirection = "asc" | "desc";

interface ContainerRow {
  key: string;
  hostKey: string;
  hostDisplayName: string;
  hostStatus: HostStatus;
  container: DockerContainer;
  stat?: DockerContainerStats;
  memoryPercent: number | null;
  health: ContainerHealth;
}

function getContainerHealth(container: DockerContainer): ContainerHealth {
  if (
    container.state !== "running"
    || container.health_status === "unhealthy"
    || container.oom_killed
  ) {
    return "attention";
  }
  if (container.state === "running") {
    return "running";
  }
  return "stopped";
}

function healthRank(health: ContainerHealth): number {
  switch (health) {
    case "attention":
      return 0;
    case "running":
      return 1;
    case "stopped":
      return 2;
  }
}

function hostStatusRank(status: HostStatus): number {
  switch (status) {
    case "online":
      return 0;
    case "pending":
      return 1;
    case "offline":
      return 2;
  }
}

function healthToneClass(health: ContainerHealth): string {
  switch (health) {
    case "attention":
      return "text-[var(--md-sys-color-error)]";
    case "running":
      return "text-[var(--md-custom-color-success)]";
    case "stopped":
      return "text-[var(--md-sys-color-outline)]";
  }
}

function compareText(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

function compareNumber(a: number, b: number): number {
  return a - b;
}

function compareStatus(a: ContainerRow, b: ContainerRow): number {
  const healthDiff = healthRank(a.health) - healthRank(b.health);
  if (healthDiff !== 0) return healthDiff;
  const hostDiff = hostStatusRank(a.hostStatus) - hostStatusRank(b.hostStatus);
  if (hostDiff !== 0) return hostDiff;
  return compareText(a.container.state, b.container.state);
}

function compareByKey(a: ContainerRow, b: ContainerRow, key: SortKey): number {
  switch (key) {
    case "container":
      return compareText(a.container.container_name, b.container.container_name);
    case "host":
      return compareText(a.hostDisplayName, b.hostDisplayName) || compareText(a.hostKey, b.hostKey);
    case "status":
      return compareStatus(a, b);
    case "cpu":
      return compareNumber(a.stat?.cpu_percent ?? -1, b.stat?.cpu_percent ?? -1);
    case "memory":
      return compareNumber(
        a.memoryPercent ?? a.stat?.memory_usage_mb ?? -1,
        b.memoryPercent ?? b.stat?.memory_usage_mb ?? -1,
      );
    case "network":
      return compareNumber(
        (a.stat?.net_rx_bytes ?? 0) + (a.stat?.net_tx_bytes ?? 0),
        (b.stat?.net_rx_bytes ?? 0) + (b.stat?.net_tx_bytes ?? 0),
      );
    case "storage":
      return compareNumber(
        (a.stat?.block_read_bytes ?? 0) + (a.stat?.block_write_bytes ?? 0),
        (b.stat?.block_read_bytes ?? 0) + (b.stat?.block_write_bytes ?? 0),
      );
    case "image":
      return compareText(a.container.image, b.container.image);
  }
}

function SortHeader({
  label,
  sortKey,
  activeKey,
  activeDirection,
  onToggle,
  ascLabel,
  descLabel,
  widthClass,
}: {
  label: string;
  sortKey: SortKey;
  activeKey: SortKey;
  activeDirection: SortDirection;
  onToggle: (key: SortKey) => void;
  ascLabel: string;
  descLabel: string;
  widthClass?: string;
}) {
  const active = activeKey === sortKey;
  const ariaSort: "ascending" | "descending" | "none" = active
    ? activeDirection === "asc" ? "ascending" : "descending"
    : "none";
  const nextLabel = active && activeDirection === "asc" ? descLabel : ascLabel;
  const Icon = active ? (activeDirection === "asc" ? ArrowUp : ArrowDown) : ArrowUpDown;

  return (
    <th className={widthClass} aria-sort={ariaSort}>
      <button
        type="button"
        aria-label={`${label} — ${nextLabel}`}
        onClick={() => onToggle(sortKey)}
        style={{
          font: "var(--md-sys-typescale-label-large)",
          transition:
            "background var(--md-sys-motion-duration-short3) var(--md-sys-motion-easing-standard), color var(--md-sys-motion-duration-short3) var(--md-sys-motion-easing-standard)",
        }}
        className={`group inline-flex w-full items-center gap-1.5 rounded-[var(--md-sys-shape-corner-small)] px-1 py-1 text-left hover:bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_8%,transparent)] focus:outline-none focus-visible:[outline:2px_solid_var(--md-sys-color-primary)] focus-visible:[outline-offset:2px] ${
          active ? "text-[var(--md-sys-color-on-surface)]" : "text-[var(--md-sys-color-on-surface-variant)]"
        }`}
      >
        <span>{label}</span>
        <Icon
          size={12}
          aria-hidden="true"
          style={{
            transition:
              "opacity var(--md-sys-motion-duration-short3) var(--md-sys-motion-easing-standard)",
          }}
          className={
            active
              ? "text-[var(--md-sys-color-on-surface)]"
              : "text-[var(--md-sys-color-outline)] opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100"
          }
        />
      </button>
    </th>
  );
}

export default function ContainersPage() {
  const { t } = useI18n();
  const { metricsMap, statusMap, isConnected } = useSSE();
  const [sortKey, setSortKey] = useState<SortKey>("status");
  const [sortDirection, setSortDirection] = useState<SortDirection>("asc");

  const { rows, total, running, attention, hostCount } = useMemo(() => {
    const collected: ContainerRow[] = [];
    let runningCount = 0;
    let attentionCount = 0;

    for (const status of Object.values(statusMap)) {
      const metrics = metricsMap[status.host_key];
      const lastSeen = metrics?.timestamp ?? status.last_seen ?? null;
      const isOnline = metrics?.is_online ?? status.is_online ?? false;
      const hostStatus = getHostStatus(lastSeen, isOnline, status.scrape_interval_secs);
      const statsByName = new Map<string, DockerContainerStats>();
      for (const stat of status.docker_stats ?? []) {
        statsByName.set(stat.container_name, stat);
      }

      for (const container of status.docker_containers ?? []) {
        const stat = statsByName.get(container.container_name);
        const memoryPercent = stat && stat.memory_limit_mb > 0
          ? (stat.memory_usage_mb / stat.memory_limit_mb) * 100
          : null;
        const health = getContainerHealth(container);
        if (health === "running") {
          runningCount += 1;
        }
        if (health === "attention") {
          attentionCount += 1;
        }
        collected.push({
          key: `${status.host_key}::${container.container_name}`,
          hostKey: status.host_key,
          hostDisplayName: metrics?.display_name ?? status.display_name,
          hostStatus,
          container,
          stat,
          memoryPercent,
          health,
        });
      }
    }

    return {
      rows: collected,
      total: collected.length,
      running: runningCount,
      attention: attentionCount,
      hostCount: new Set(collected.map((row) => row.hostKey)).size,
    };
  }, [metricsMap, statusMap]);

  const sortedRows = useMemo(() => {
    const factor = sortDirection === "asc" ? 1 : -1;
    return [...rows].sort((a, b) => {
      const primary = compareByKey(a, b, sortKey);
      if (primary !== 0) return primary * factor;
      return compareText(a.container.container_name, b.container.container_name);
    });
  }, [rows, sortDirection, sortKey]);

  const isLoading = !isConnected && Object.keys(statusMap).length === 0;

  const handleToggleSort = (key: SortKey) => {
    if (key === sortKey) {
      setSortDirection((dir) => (dir === "asc" ? "desc" : "asc"));
      return;
    }
    setSortKey(key);
    setSortDirection(key === "container" || key === "host" || key === "image" ? "asc" : "desc");
  };

  return (
    <div className="page-content fade-in">
      <PageHeader
        icon={<Box size={18} aria-hidden="true" />}
        title={t.containers.title}
        badge={total}
        description={t.containers.description}
        right={
          (total > 0 || attention > 0) ? (
            <div className="page-header__stats">
              <span className="page-header__stats-item">
                {t.containers.summary.total.replace("{count}", String(total))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.running.replace("{count}", String(running))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.attention.replace("{count}", String(attention))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.hosts.replace("{count}", String(hostCount))}
              </span>
            </div>
          ) : undefined
        }
      />

      <div className="glass-card overflow-hidden">
        {isLoading && (
          <div className="p-5">
            {[1, 2, 3].map((i) => (
              <div key={i} className="skeleton mb-2 h-12 last:mb-0" />
            ))}
          </div>
        )}

        {!isLoading && rows.length === 0 && (
          <div className="px-6 py-12 text-center text-[var(--md-sys-color-on-surface-variant)]">
            <Activity size={36} className="mx-auto mb-3 opacity-30" />
            <div
              className="mb-1.5 text-[var(--md-sys-color-on-surface)]"
              style={{ font: "var(--md-sys-typescale-title-medium)" }}
            >
              {t.containers.noContainers}
            </div>
            <div style={{ font: "var(--md-sys-typescale-body-medium)" }}>
              {t.containers.noContainersHint}
            </div>
          </div>
        )}

        {!isLoading && rows.length > 0 && (
          <div className="systems-table-wrap">
            <table className="systems-table">
              <thead>
                <tr>
                  <SortHeader
                    label={t.containers.tableHeaders.container}
                    sortKey="container"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.host}
                    sortKey="host"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.status}
                    sortKey="status"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.cpu}
                    sortKey="cpu"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                    widthClass="w-[14%]"
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.memory}
                    sortKey="memory"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                    widthClass="w-[16%]"
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.network}
                    sortKey="network"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.storage}
                    sortKey="storage"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                  <SortHeader
                    label={t.containers.tableHeaders.image}
                    sortKey="image"
                    activeKey={sortKey}
                    activeDirection={sortDirection}
                    onToggle={handleToggleSort}
                    ascLabel={t.containers.sortAsc}
                    descLabel={t.containers.sortDesc}
                  />
                </tr>
              </thead>
              <tbody>
                {sortedRows.map((row) => {
                  const { container, stat } = row;
                  const healthClass = healthToneClass(row.health);

                  const titleStyle = { font: "var(--md-sys-typescale-title-small)" };
                  const bodySmallStyle = { font: "var(--md-sys-typescale-body-small)" };
                  const labelSmallStyle = { font: "var(--md-sys-typescale-label-small)" };
                  const labelMedStyle = { font: "var(--md-sys-typescale-label-medium)" };
                  const monoLabelSmall = {
                    font: "var(--md-sys-typescale-label-small)",
                    fontFamily: "var(--font-mono), monospace",
                  };

                  return (
                    <tr key={row.key}>
                      <td>
                        <div className="grid min-w-0 gap-1 rounded-[var(--md-sys-shape-corner-medium)] px-1 py-0.5">
                          <div
                            className="truncate whitespace-nowrap text-[var(--md-sys-color-on-surface)]"
                            style={titleStyle}
                            title={container.container_name}
                          >
                            {container.container_name}
                          </div>
                          <div
                            className="flex flex-wrap gap-1.5 text-[var(--md-sys-color-outline)]"
                            style={labelSmallStyle}
                          >
                            {container.compose_project && <span>{container.compose_project}</span>}
                            {container.compose_service && <span>{container.compose_service}</span>}
                            {!container.compose_project && !container.compose_service && (
                              <span>{t.dockerGrid.standalone}</span>
                            )}
                          </div>
                        </div>
                      </td>
                      <td>
                        <div className="grid min-w-0 gap-1 rounded-[var(--md-sys-shape-corner-medium)] px-1 py-0.5">
                          <div className="flex items-center gap-2">
                            <span
                              className={STATUS_DOT_CLASS[row.hostStatus]}
                              style={{ width: 8, height: 8, flexShrink: 0 }}
                            />
                            <Link
                              href={`/host/?key=${encodeURIComponent(row.hostKey)}`}
                              prefetch={false}
                              className="truncate whitespace-nowrap text-[var(--md-sys-color-on-surface)] no-underline"
                              style={titleStyle}
                            >
                              {row.hostDisplayName}
                            </Link>
                          </div>
                          <div
                            className="truncate whitespace-nowrap text-[var(--md-sys-color-outline)]"
                            style={monoLabelSmall}
                            title={row.hostKey}
                          >
                            {row.hostKey}
                          </div>
                        </div>
                      </td>
                      <td>
                        <div className="grid gap-1">
                          <div
                            className={`uppercase leading-tight ${healthClass}`}
                            style={labelMedStyle}
                          >
                            {container.state}
                          </div>
                          {container.health_status && (
                            <div
                              className="leading-tight text-[var(--md-sys-color-outline)]"
                              style={labelSmallStyle}
                            >
                              {container.health_status}
                            </div>
                          )}
                          <div
                            className="flex flex-wrap gap-1.5 text-[var(--md-sys-color-outline)]"
                            style={labelSmallStyle}
                          >
                            {container.oom_killed && (
                              <span className="text-[var(--md-sys-color-error)]">
                                {t.containers.flags.oom}
                              </span>
                            )}
                            {container.exit_code !== null && container.exit_code !== undefined && (
                              <span>
                                {t.containers.flags.exit.replace(
                                  "{code}",
                                  String(container.exit_code),
                                )}
                              </span>
                            )}
                            {container.restart_count > 0 && (
                              <span>
                                {t.containers.flags.restarts.replace(
                                  "{count}",
                                  String(container.restart_count),
                                )}
                              </span>
                            )}
                          </div>
                        </div>
                      </td>
                      <td>
                        {stat ? (
                          <InlineMeter value={stat.cpu_percent} label="CPU" />
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-outline)]"
                            style={bodySmallStyle}
                          >
                            {t.containers.noLiveStats}
                          </span>
                        )}
                      </td>
                      <td>
                        {stat ? (
                          <div className="grid gap-1.5">
                            {row.memoryPercent !== null ? (
                              <InlineMeter value={row.memoryPercent} label="MEM" />
                            ) : (
                              <div
                                className="tabular-nums text-[var(--md-sys-color-on-surface)]"
                                style={labelMedStyle}
                              >
                                {stat.memory_usage_mb} MB
                              </div>
                            )}
                            <div
                              className="text-[var(--md-sys-color-outline)]"
                              style={monoLabelSmall}
                            >
                              {stat.memory_limit_mb > 0
                                ? `${stat.memory_usage_mb} / ${stat.memory_limit_mb} MB`
                                : `${stat.memory_usage_mb} MB`}
                            </div>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-outline)]"
                            style={bodySmallStyle}
                          >
                            {t.containers.noLiveStats}
                          </span>
                        )}
                      </td>
                      <td>
                        {stat ? (
                          <div className="grid gap-1" style={monoLabelSmall}>
                            <span>
                              <span className="text-[var(--md-sys-color-outline)]">RX </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.net_rx_bytes)}
                              </span>
                            </span>
                            <span>
                              <span className="text-[var(--md-sys-color-outline)]">TX </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.net_tx_bytes)}
                              </span>
                            </span>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-outline)]"
                            style={bodySmallStyle}
                          >
                            {t.containers.noLiveStats}
                          </span>
                        )}
                      </td>
                      <td>
                        {stat && (stat.block_read_bytes > 0 || stat.block_write_bytes > 0) ? (
                          <div className="grid gap-1" style={monoLabelSmall}>
                            <span>
                              <span className="text-[var(--md-sys-color-outline)]">R </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.block_read_bytes)}
                              </span>
                            </span>
                            <span>
                              <span className="text-[var(--md-sys-color-outline)]">W </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.block_write_bytes)}
                              </span>
                            </span>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-outline)]"
                            style={bodySmallStyle}
                          >
                            {t.containers.noIo}
                          </span>
                        )}
                      </td>
                      <td>
                        <div className="grid min-w-0 gap-1.5">
                          <div
                            className="truncate whitespace-nowrap text-[var(--md-sys-color-on-surface)]"
                            style={monoLabelSmall}
                            title={container.image}
                          >
                            {container.image}
                          </div>
                          <div
                            className="flex items-center gap-1.5 text-[var(--md-sys-color-outline)]"
                            style={labelSmallStyle}
                          >
                            <Server size={12} />
                            <span>{container.status}</span>
                          </div>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
