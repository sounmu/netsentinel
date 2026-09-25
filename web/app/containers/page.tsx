"use client";

import Link from "next/link";
import { Fragment, useMemo, useState } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Box, RotateCcw, Search, Server } from "lucide-react";
import { PageHeader } from "@/app/components/PageHeader";
import { useI18n } from "@/app/i18n/I18nContext";
import {
  useSSEConnection,
  useSSEMetricsMap,
  useSSEStatusMap,
} from "@/app/lib/sse-context";
import { getCurrentDockerStats } from "@/app/lib/docker-stats";
import {
  assessContainer,
  filterContainerRecords,
  groupContainerRecords,
  operationalCategoryRank,
  summarizeContainers,
  type ContainerGroupMode,
  type ContainerOperationRecord,
  type ContainerOperationalCategory,
} from "@/app/lib/container-operations";
import { formatBytes } from "@/app/lib/formatters";
import {
  getHostStatus,
  STATUS_DOT_CLASS,
  type HostStatus,
} from "@/app/lib/status";
import type { DockerContainerStats } from "@/app/types/metrics";

type SortKey = "container" | "host" | "status" | "cpu" | "memory" | "network" | "storage" | "image";
type SortDirection = "asc" | "desc";

interface ContainerRow extends ContainerOperationRecord {
  hostStatus: HostStatus;
  stat?: DockerContainerStats;
  memoryPercent: number | null;
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

function categoryToneClass(category: ContainerOperationalCategory): string {
  switch (category) {
    case "attention":
      return "text-[var(--md-sys-color-error)]";
    case "running":
      return "text-[var(--md-custom-color-success)]";
    case "inactive":
      return "text-[var(--md-custom-color-warning)]";
    case "completed":
      return "text-[var(--md-sys-color-on-surface-variant)]";
  }
}

function compareText(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

function compareNumber(a: number, b: number): number {
  return a - b;
}

function compareStatus(a: ContainerRow, b: ContainerRow): number {
  const categoryDiff = operationalCategoryRank(a.assessment.category)
    - operationalCategoryRank(b.assessment.category);
  if (categoryDiff !== 0) return categoryDiff;
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
        className={`group inline-flex min-h-12 w-full items-center gap-1.5 rounded-[var(--md-sys-shape-corner-small)] px-1 py-1 text-left hover:bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_8%,transparent)] focus:outline-none focus-visible:[outline:2px_solid_var(--md-sys-color-primary)] focus-visible:[outline-offset:2px] ${
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
  const metricsMap = useSSEMetricsMap();
  const statusMap = useSSEStatusMap();
  const isConnected = useSSEConnection();
  const [sortKey, setSortKey] = useState<SortKey>("status");
  const [sortDirection, setSortDirection] = useState<SortDirection>("asc");
  const [query, setQuery] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<
    ContainerOperationalCategory | "all"
  >("all");
  const [observedStateFilter, setObservedStateFilter] = useState("all");
  const [groupMode, setGroupMode] = useState<ContainerGroupMode>("stack");

  const { rows, hostCount } = useMemo(() => {
    const collected: ContainerRow[] = [];

    for (const status of Object.values(statusMap)) {
      const metrics = metricsMap[status.host_key];
      const lastSeen = metrics?.timestamp ?? status.last_seen ?? null;
      const isOnline = metrics?.is_online ?? status.is_online ?? false;
      const hostStatus = getHostStatus(lastSeen, isOnline, status.scrape_interval_secs);
      const statsByName = new Map<string, DockerContainerStats>();
      for (const stat of getCurrentDockerStats(metrics, status)) {
        statsByName.set(stat.container_name, stat);
      }

      for (const container of status.docker_containers ?? []) {
        const stat = statsByName.get(container.container_name);
        const memoryPercent = stat && stat.memory_limit_mb > 0
          ? (stat.memory_usage_mb / stat.memory_limit_mb) * 100
          : null;
        collected.push({
          key: `${status.host_key}::${container.container_name}`,
          hostKey: status.host_key,
          hostDisplayName: metrics?.display_name ?? status.display_name,
          hostStatus,
          container,
          stat,
          memoryPercent,
          assessment: assessContainer(container),
        });
      }
    }

    return {
      rows: collected,
      hostCount: new Set(collected.map((row) => row.hostKey)).size,
    };
  }, [metricsMap, statusMap]);

  const summary = useMemo(
    () => summarizeContainers(rows.map((row) => row.container)),
    [rows],
  );

  const observedStates = useMemo(
    () => [...new Set(rows.map((row) => row.container.state.toLowerCase()))].sort(compareText),
    [rows],
  );

  const filteredRows = useMemo(
    () => filterContainerRecords(rows, {
      query,
      category: categoryFilter,
      observedState: observedStateFilter,
    }),
    [rows, query, categoryFilter, observedStateFilter],
  );

  const sortedRows = useMemo(() => {
    const factor = sortDirection === "asc" ? 1 : -1;
    return [...filteredRows].sort((a, b) => {
      const primary = compareByKey(a, b, sortKey);
      if (primary !== 0) return primary * factor;
      return compareText(a.container.container_name, b.container.container_name);
    });
  }, [filteredRows, sortDirection, sortKey]);

  const groupedRows = useMemo(
    () => groupContainerRecords(sortedRows, groupMode),
    [sortedRows, groupMode],
  );

  const isLoading = !isConnected && Object.keys(statusMap).length === 0;

  const handleToggleSort = (key: SortKey) => {
    if (key === sortKey) {
      setSortDirection((dir) => (dir === "asc" ? "desc" : "asc"));
      return;
    }
    setSortKey(key);
    setSortDirection(key === "container" || key === "host" || key === "image" ? "asc" : "desc");
  };

  const resetFilters = () => {
    setQuery("");
    setCategoryFilter("all");
    setObservedStateFilter("all");
  };

  return (
    <div className="page-content fade-in">
      <PageHeader
        icon={<Box size={18} aria-hidden="true" />}
        title={t.containers.title}
        badge={summary.total}
        description={t.containers.description}
        right={
          summary.total > 0 ? (
            <div className="page-header__stats">
              <span className="page-header__stats-item">
                {t.containers.summary.total.replace("{count}", String(summary.total))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.running.replace("{count}", String(summary.running))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.attention.replace("{count}", String(summary.attention))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.inactive.replace("{count}", String(summary.inactive))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.completed.replace("{count}", String(summary.completed))}
              </span>
              <span className="page-header__stats-item">
                {t.containers.summary.hosts.replace("{count}", String(hostCount))}
              </span>
            </div>
          ) : undefined
        }
      />

      <div className="container-observation-note" role="note">
        <div>
          <strong>{t.containers.observation.title}</strong>
          <p>{t.containers.observation.description}</p>
        </div>
        <Link href="/alerts?tab=rules" className="md-btn-tonal">
          {t.containers.observation.reviewRules}
        </Link>
      </div>

      <div className="container-operations-toolbar" aria-label={t.containers.filters.title}>
        <label className="container-filter container-filter--search">
          <span>{t.containers.filters.search}</span>
          <span className="container-search-input">
            <Search size={16} aria-hidden="true" />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t.containers.filters.searchPlaceholder}
            />
          </span>
        </label>

        <label className="container-filter">
          <span>{t.containers.filters.operational}</span>
          <select
            value={categoryFilter}
            onChange={(event) => setCategoryFilter(
              event.target.value as ContainerOperationalCategory | "all",
            )}
          >
            <option value="all">{t.containers.filters.allOperational}</option>
            <option value="attention">{t.containers.categories.attention}</option>
            <option value="running">{t.containers.categories.running}</option>
            <option value="inactive">{t.containers.categories.inactive}</option>
            <option value="completed">{t.containers.categories.completed}</option>
          </select>
        </label>

        <label className="container-filter">
          <span>{t.containers.filters.observed}</span>
          <select
            value={observedStateFilter}
            onChange={(event) => setObservedStateFilter(event.target.value)}
          >
            <option value="all">{t.containers.filters.allObserved}</option>
            {observedStates.map((state) => (
              <option key={state} value={state}>{state}</option>
            ))}
          </select>
        </label>

        <label className="container-filter">
          <span>{t.containers.filters.groupBy}</span>
          <select
            value={groupMode}
            onChange={(event) => setGroupMode(event.target.value as ContainerGroupMode)}
          >
            <option value="stack">{t.containers.filters.groupStack}</option>
            <option value="service">{t.containers.filters.groupService}</option>
            <option value="none">{t.containers.filters.groupNone}</option>
          </select>
        </label>

        <button type="button" className="md-btn-tonal" onClick={resetFilters}>
          <RotateCcw size={16} aria-hidden="true" />
          {t.containers.filters.reset}
        </button>

        <div className="container-filter-result" aria-live="polite">
          {t.containers.filters.results
            .replace("{visible}", String(filteredRows.length))
            .replace("{total}", String(rows.length))}
        </div>
      </div>

      <div className="glass-card overflow-hidden">
        {isLoading && (
          <div className="p-5">
            {[1, 2, 3].map((i) => (
              <div key={i} className="skeleton mb-2 h-12 last:mb-0" />
            ))}
          </div>
        )}

        {!isLoading && rows.length === 0 && (
          <div
            style={{
              padding: "48px 24px",
              textAlign: "center",
              color: "var(--text-muted)",
            }}
          >
            <Box size={40} style={{ margin: "0 auto 12px", opacity: 0.3 }} />
            <div
              style={{
                fontSize: 15,
                fontWeight: 600,
                marginBottom: 6,
                color: "var(--text-primary)",
              }}
            >
              {t.containers.noContainers}
            </div>
            <div style={{ fontSize: 13 }}>{t.containers.noContainersHint}</div>
          </div>
        )}

        {!isLoading && rows.length > 0 && filteredRows.length === 0 && (
          <div className="container-filter-empty">
            <strong>{t.containers.filters.noResults}</strong>
            <span>{t.containers.filters.noResultsHint}</span>
            <button type="button" className="md-btn-tonal" onClick={resetFilters}>
              <RotateCcw size={16} aria-hidden="true" />
              {t.containers.filters.reset}
            </button>
          </div>
        )}

        {!isLoading && filteredRows.length > 0 && (
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
                {groupedRows.map((group) => {
                  const groupSummary = summarizeContainers(
                    group.rows.map((row) => row.container),
                  );
                  return (
                    <Fragment key={group.key}>
                      {groupMode !== "none" && (
                        <tr className="container-group-row">
                          <td colSpan={8}>
                            <div>
                              <strong>
                                {group.key
                                  .replace("standalone", t.containers.groups.standalone)
                                  .replace("unlabelled", t.containers.groups.unlabelled)}
                              </strong>
                              <span>
                                {t.containers.groups.summary
                                  .replace("{total}", String(groupSummary.total))
                                  .replace("{attention}", String(groupSummary.attention))
                                  .replace("{running}", String(groupSummary.running))}
                              </span>
                            </div>
                          </td>
                        </tr>
                      )}
                      {group.rows.map((row) => {
                        const { container, stat, assessment } = row;
                        const categoryClass = categoryToneClass(assessment.category);
                        const reason = assessment.attentionReason
                          ? t.containers.reasons[assessment.attentionReason].replace(
                            "{code}",
                            String(container.exit_code ?? ""),
                          )
                          : assessment.inactiveReason
                            ? t.containers.reasons[assessment.inactiveReason]
                            : null;

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
                            className="flex flex-wrap gap-1.5 text-[var(--md-sys-color-on-surface-variant)]"
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
                              className="inline-flex min-h-12 items-center truncate whitespace-nowrap text-[var(--md-sys-color-on-surface)] no-underline"
                              style={titleStyle}
                            >
                              {row.hostDisplayName}
                            </Link>
                          </div>
                          <div
                            className="truncate whitespace-nowrap text-[var(--md-sys-color-on-surface-variant)]"
                            style={monoLabelSmall}
                            title={row.hostKey}
                          >
                            {row.hostKey}
                          </div>
                        </div>
                      </td>
                      <td>
                        <div className="grid gap-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <span
                              className="uppercase leading-tight text-[var(--md-sys-color-on-surface)]"
                              style={labelMedStyle}
                            >
                              {container.state}
                            </span>
                            <span
                              className={`container-category-badge ${categoryClass}`}
                              style={labelSmallStyle}
                            >
                              {t.containers.categories[assessment.category]}
                            </span>
                          </div>
                          {reason && (
                            <div
                              className={`leading-tight ${categoryClass}`}
                              style={labelSmallStyle}
                            >
                              {reason}
                            </div>
                          )}
                          {container.health_status && (
                            <div
                              className="leading-tight text-[var(--md-sys-color-on-surface-variant)]"
                              style={labelSmallStyle}
                            >
                              {container.health_status}
                            </div>
                          )}
                          <div
                            className="flex flex-wrap gap-1.5 text-[var(--md-sys-color-on-surface-variant)]"
                            style={labelSmallStyle}
                          >
                            {container.oom_killed && (
                              <span className="text-[var(--md-sys-color-error)]">
                                {t.containers.flags.oom}
                              </span>
                            )}
                            {container.exit_code !== null
                              && container.exit_code !== undefined
                              && (container.state.toLowerCase() !== "running" || container.exit_code !== 0)
                              && (
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
                          <Link
                            href={`/host/?key=${encodeURIComponent(row.hostKey)}`}
                            prefetch={false}
                            className="container-inspect-link"
                          >
                            {t.containers.inspectHost}
                          </Link>
                        </div>
                      </td>
                      <td>
                        {stat ? (
                          <span
                            className="tabular-nums"
                            style={titleStyle}
                          >
                            {stat.cpu_percent.toFixed(1)}%
                          </span>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-on-surface-variant)]"
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
                              <span
                                className="tabular-nums"
                                style={titleStyle}
                              >
                                {row.memoryPercent.toFixed(1)}%
                              </span>
                            ) : (
                              <div
                                className="tabular-nums text-[var(--md-sys-color-on-surface)]"
                                style={titleStyle}
                              >
                                {stat.memory_usage_mb} MB
                              </div>
                            )}
                            <div
                              className="whitespace-nowrap text-[var(--md-sys-color-on-surface-variant)]"
                              style={monoLabelSmall}
                            >
                              {stat.memory_limit_mb > 0
                                ? `${stat.memory_usage_mb} / ${stat.memory_limit_mb} MB`
                                : `${stat.memory_usage_mb} MB`}
                            </div>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-on-surface-variant)]"
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
                              <span className="text-[var(--md-sys-color-on-surface-variant)]">RX </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.net_rx_bytes)}
                              </span>
                            </span>
                            <span>
                              <span className="text-[var(--md-sys-color-on-surface-variant)]">TX </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.net_tx_bytes)}
                              </span>
                            </span>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-on-surface-variant)]"
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
                              <span className="text-[var(--md-sys-color-on-surface-variant)]">R </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.block_read_bytes)}
                              </span>
                            </span>
                            <span>
                              <span className="text-[var(--md-sys-color-on-surface-variant)]">W </span>
                              <span className="text-[var(--md-sys-color-on-surface)]">
                                {formatBytes(stat.block_write_bytes)}
                              </span>
                            </span>
                          </div>
                        ) : (
                          <span
                            className="text-[var(--md-sys-color-on-surface-variant)]"
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
                            className="flex items-center gap-1.5 text-[var(--md-sys-color-on-surface-variant)]"
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
                    </Fragment>
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
