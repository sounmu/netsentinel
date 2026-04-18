"use client";

import useSWR from "swr";
import type { AlertConfigRow } from "@/app/lib/api";
import { fetcher, getHostAlertConfigsUrl } from "@/app/lib/api";
import type { HostSummary } from "@/app/types/metrics";
import { useI18n } from "@/app/i18n/I18nContext";
import type { MetricPrefix } from "./shared";

interface Props {
  hosts: HostSummary[];
  globalConfigs: AlertConfigRow[];
  selected: Set<string>;
  onToggle: (hostKey: string, checked: boolean) => void;
}

export function RulesMatrix({ hosts, globalConfigs, selected, onToggle }: Props) {
  const { t } = useI18n();

  if (hosts.length === 0) {
    return <div className="alerts-card alerts-card--empty">{t.alerts.noHosts}</div>;
  }

  return (
    <div className="alerts-matrix-wrap" role="region" aria-label={t.alerts.rules.matrix}>
      <table className="alerts-matrix">
        <thead>
          <tr>
            <th scope="col" className="alerts-matrix__select" aria-label="select" />
            <th scope="col" className="alerts-matrix__host">
              {t.common.host}
            </th>
            <th scope="col">{t.alerts.cpu}</th>
            <th scope="col">{t.alerts.memory}</th>
            <th scope="col">{t.alerts.disk}</th>
          </tr>
        </thead>
        <tbody>
          {hosts.map((host) => (
            <MatrixRow
              key={host.host_key}
              host={host}
              globalConfigs={globalConfigs}
              selected={selected.has(host.host_key)}
              onToggle={onToggle}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function MatrixRow({
  host,
  globalConfigs,
  selected,
  onToggle,
}: {
  host: HostSummary;
  globalConfigs: AlertConfigRow[];
  selected: boolean;
  onToggle: (hostKey: string, checked: boolean) => void;
}) {
  const { data: hostConfigs } = useSWR<AlertConfigRow[]>(
    getHostAlertConfigsUrl(host.host_key),
    fetcher,
    { revalidateOnFocus: false, shouldRetryOnError: false },
  );

  const cpu = resolveConfig(hostConfigs, globalConfigs, "cpu");
  const memory = resolveConfig(hostConfigs, globalConfigs, "memory");
  const disk = resolveConfig(hostConfigs, globalConfigs, "disk");

  return (
    <tr>
      <td className="alerts-matrix__select">
        <input
          type="checkbox"
          className="alerts-matrix__checkbox"
          checked={selected}
          onChange={(e) => onToggle(host.host_key, e.target.checked)}
          aria-label={`Select ${host.display_name}`}
        />
      </td>
      <th scope="row" className="alerts-matrix__host">
        <div className="alerts-matrix__host-name">{host.display_name}</div>
        <div className="alerts-matrix__host-key">{host.host_key}</div>
      </th>
      <MatrixCell cell={cpu} />
      <MatrixCell cell={memory} />
      <MatrixCell cell={disk} />
    </tr>
  );
}

interface ResolvedCell {
  threshold: number;
  enabled: boolean;
  overridden: boolean;
}

function resolveConfig(
  hostConfigs: AlertConfigRow[] | undefined,
  globalConfigs: AlertConfigRow[],
  metric: MetricPrefix,
): ResolvedCell | null {
  const hostOverride = hostConfigs?.find((c) => c.metric_type === metric);
  if (hostOverride) {
    return { threshold: hostOverride.threshold, enabled: hostOverride.enabled, overridden: true };
  }
  const g = globalConfigs.find((c) => c.metric_type === metric);
  if (g) return { threshold: g.threshold, enabled: g.enabled, overridden: false };
  return null;
}

function MatrixCell({ cell }: { cell: ResolvedCell | null }) {
  if (!cell) return <td className="alerts-matrix__cell alerts-matrix__cell--disabled">—</td>;
  const className = [
    "alerts-matrix__cell",
    cell.overridden ? "alerts-matrix__cell--override" : "",
    cell.enabled ? "" : "alerts-matrix__cell--disabled",
  ]
    .filter(Boolean)
    .join(" ");
  return <td className={className}>{cell.enabled ? `${cell.threshold}%` : "off"}</td>;
}
