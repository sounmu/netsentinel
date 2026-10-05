"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { Box, Search } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import {
  assessContainer,
  operationalCategoryRank,
  summarizeContainers,
  type ContainerAssessment,
  type ContainerOperationalCategory,
} from "@/app/lib/container-operations";
import { formatBytes } from "@/app/lib/formatters";
import type { DockerContainer, DockerContainerStats } from "@/app/types/metrics";
import { EmptyState } from "@/app/components/ui";

interface DockerGridProps {
  containers: readonly DockerContainer[];
  /** Live per-container stats (SSE `metrics` falling back to `status`). */
  stats?: readonly DockerContainerStats[];
}

type FilterMode = "all" | ContainerOperationalCategory;

interface Row {
  container: DockerContainer;
  assessment: ContainerAssessment;
  stat: DockerContainerStats | undefined;
}

const FILTER_ORDER: readonly ContainerOperationalCategory[] = [
  "running",
  "attention",
  "inactive",
  "completed",
];

/** Search box only earns its space once the list stops fitting at a glance. */
const SEARCH_THRESHOLD = 8;

const MB = 1024 * 1024;

function compareText(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

export default function DockerGrid({ containers, stats = [] }: DockerGridProps) {
  const { t } = useI18n();
  const [filter, setFilter] = useState<FilterMode>("all");
  const [query, setQuery] = useState("");

  const summary = useMemo(() => summarizeContainers(containers), [containers]);

  const rows = useMemo<Row[]>(() => {
    const statsByName = new Map(stats.map((s) => [s.container_name, s]));
    return containers.map((container) => ({
      container,
      assessment: assessContainer(container),
      stat: statsByName.get(container.container_name),
    }));
  }, [containers, stats]);

  // A live update can empty the selected bucket (e.g. the one failing
  // container recovers). Fall back to "all" instead of showing an empty list.
  const activeFilter: FilterMode =
    filter !== "all" && summary[filter] === 0 ? "all" : filter;

  const groups = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    const visible = rows.filter(({ container, assessment }) => {
      if (activeFilter !== "all" && assessment.category !== activeFilter) return false;
      if (!needle) return true;
      return [container.container_name, container.image, container.compose_service]
        .some((v) => v?.toLocaleLowerCase().includes(needle));
    });

    const map = new Map<string, Row[]>();
    for (const row of visible) {
      const key = row.container.compose_project?.trim() || "";
      const group = map.get(key);
      if (group) group.push(row);
      else map.set(key, [row]);
    }
    for (const group of map.values()) {
      group.sort(
        (a, b) =>
          operationalCategoryRank(a.assessment.category)
            - operationalCategoryRank(b.assessment.category)
          || compareText(a.container.container_name, b.container.container_name),
      );
    }
    // Named stacks alphabetically, standalone containers last.
    return [...map.entries()].sort(([a], [b]) => {
      if (!a !== !b) return a ? -1 : 1;
      return compareText(a, b);
    });
  }, [rows, activeFilter, query]);

  if (containers.length === 0) {
    return (
      <EmptyState
        icon={<Box size={24} aria-hidden="true" />}
        title={t.dockerGrid.noContainers}
      />
    );
  }

  const showSearch = containers.length > SEARCH_THRESHOLD;
  const hasGroups = groups.length > 1 || (groups.length === 1 && groups[0][0] !== "");

  return (
    <div className="ctr-list">
      <div className="ctr-list__toolbar">
        <div className="segmented" role="group" aria-label={t.host.dockerContainers}>
          <button
            type="button"
            className="segmented__item"
            aria-pressed={activeFilter === "all"}
            onClick={() => setFilter("all")}
          >
            {t.dockerGrid.all}
            <span className="segmented__count">{summary.total}</span>
          </button>
          {FILTER_ORDER.filter((c) => summary[c] > 0).map((category) => (
            <button
              key={category}
              type="button"
              className="segmented__item"
              data-category={category}
              aria-pressed={activeFilter === category}
              onClick={() => setFilter(category)}
            >
              <span className="ctr-list__dot" data-category={category} aria-hidden="true" />
              {t.containers.categories[category]}
              <span className="segmented__count">{summary[category]}</span>
            </button>
          ))}
        </div>

        {showSearch && (
          <label className="ctr-list__search">
            <Search size={14} aria-hidden="true" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t.dockerGrid.searchPlaceholder}
              aria-label={t.dockerGrid.searchPlaceholder}
            />
          </label>
        )}
      </div>

      <div className="ctr-list__table" role="table" aria-label={t.host.dockerContainers}>
        <div className="ctr-list__head" role="row">
          <span role="columnheader">{t.dockerGrid.colContainer}</span>
          <span role="columnheader">{t.dockerGrid.colStatus}</span>
          <span role="columnheader">CPU</span>
          <span role="columnheader">{t.dockerGrid.colMemory}</span>
        </div>

        {groups.length === 0 && (
          <div className="ctr-list__no-match">{t.dockerGrid.noFilterMatches}</div>
        )}

        {groups.map(([project, groupRows]) => {
          const groupSummary = summarizeContainers(groupRows.map((r) => r.container));
          return (
            <div key={project || "__standalone"} role="rowgroup" className="ctr-list__group">
              {hasGroups && (
                <div className="ctr-list__group-head" role="row">
                  <span role="cell" className="ctr-list__group-name">
                    {project || t.dockerGrid.standalone}
                  </span>
                  <span role="cell" className="ctr-list__group-meta">
                    {t.dockerGrid.groupRunning
                      .replace("{running}", String(groupSummary.running))
                      .replace("{total}", String(groupSummary.total))}
                    {groupSummary.attention > 0 && (
                      <span className="ctr-list__group-alert">
                        {t.dockerGrid.groupAttention.replace(
                          "{count}",
                          String(groupSummary.attention),
                        )}
                      </span>
                    )}
                  </span>
                </div>
              )}
              {groupRows.map((row) => (
                <ContainerRow key={row.container.container_name} row={row} />
              ))}
            </div>
          );
        })}
      </div>

      <p className="ctr-list__footnote">
        {t.dockerGrid.observationNote}{" "}
        <Link href="/alerts?tab=rules">{t.dockerGrid.reviewRules}</Link>
      </p>
    </div>
  );
}

function ContainerRow({ row }: { row: Row }) {
  const { t } = useI18n();
  const { container, assessment, stat } = row;
  const category = assessment.category;

  const reasonKey = assessment.attentionReason ?? assessment.inactiveReason;
  const reason = reasonKey
    ? t.containers.reasons[reasonKey].replace("{code}", String(container.exit_code ?? ""))
    : null;

  const meta: string[] = [];
  if (
    container.health_status &&
    assessment.attentionReason !== "unhealthy" &&
    !container.status.toLowerCase().includes(container.health_status.toLowerCase())
  ) {
    meta.push(container.health_status);
  }
  if (container.restart_count > 0) {
    meta.push(t.dockerGrid.restarts.replace("{count}", String(container.restart_count)));
  }
  if (container.oom_killed && assessment.attentionReason !== "oom") {
    meta.push("OOM");
  }

  const memPct = stat && stat.memory_limit_mb > 0
    ? (stat.memory_usage_mb / stat.memory_limit_mb) * 100
    : null;
  // Live stats only mean something for a live process; a stopped
  // container can still carry its last sample.
  const showStats = stat !== undefined && container.state.toLowerCase() === "running";

  return (
    <div
      className="ctr-list__row"
      role="row"
      data-category={category}
      data-live={showStats}
    >
      <div role="cell" className="ctr-list__cell-name">
        <div className="ctr-list__name-wrap">
          <div className="ctr-list__name" title={container.container_name}>
            {container.container_name}
          </div>
          <div className="ctr-list__image" title={container.image}>
            {container.image}
          </div>
        </div>
      </div>

      <div role="cell" className="ctr-list__cell-status">
        <span className="ctr-list__status" data-category={category}>
          <span className="ctr-list__dot" data-category={category} aria-hidden="true" />
          {t.containers.categories[category]}
        </span>
        <div className="ctr-list__status-detail" title={container.status}>
          {container.status}
        </div>
        {(reason || meta.length > 0) && (
          <div className="ctr-list__substatus">
            {reason && (
              <span className="ctr-list__reason" data-category={category}>
                {reason}
              </span>
            )}
            {meta.map((m) => (
              <span key={m} data-health={m === container.health_status ? m : undefined}>
                {m}
              </span>
            ))}
          </div>
        )}
      </div>

      <div role="cell" className="ctr-list__cell-metric">
        <span className="ctr-list__metric-label">CPU</span>
        {showStats ? (
          <span className="ctr-list__metric-value">
            {stat.cpu_percent.toFixed(1)}%
          </span>
        ) : (
          <span className="ctr-list__metric-empty">—</span>
        )}
      </div>

      <div role="cell" className="ctr-list__cell-metric">
        <span className="ctr-list__metric-label">{t.dockerGrid.colMemory}</span>
        {showStats ? (
          <>
            <span className="ctr-list__metric-value">
              {formatBytes(stat.memory_usage_mb * MB)}
              {stat.memory_limit_mb > 0 && (
                <span className="ctr-list__metric-limit">
                  {" / "}
                  {formatBytes(stat.memory_limit_mb * MB)}
                </span>
              )}
            </span>
            {memPct !== null && (
              <span className="ctr-list__metric-percent">
                {memPct.toFixed(1)}%
              </span>
            )}
          </>
        ) : (
          <span className="ctr-list__metric-empty">—</span>
        )}
      </div>
    </div>
  );
}
