"use client";

import { useEffect, useState } from "react";
import useSWR from "swr";
import { CheckCircle, XCircle, Shield } from "lucide-react";
import {
  ApiError,
  PublicMonitorStatus,
  PublicHostStatus,
  PublicStatusResponse,
  getPublicStatusUrl,
  publicFetcher,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import { uptimeTone } from "@/app/lib/status";
import { PageHeader } from "@/app/components/PageHeader";
import { Panel, EmptyState, StatusDot, Badge } from "@/app/components/ui";

type StatusRow = {
  key: string;
  primary: string;
  secondary: string;
  is_online: boolean;
  uptime: number;
  uptimeLabel: string;
};

export default function StatusPage() {
  const { t, locale } = useI18n();
  const { data, error, isLoading } = useSWR<PublicStatusResponse>(
    getPublicStatusUrl(), publicFetcher,
    { refreshInterval: 30000, shouldRetryOnError: false }
  );

  // `/api/public/status` returns 404 when `PUBLIC_STATUS_ENABLED` is unset.
  // That is a configuration signal, not a failure, so surface it explicitly
  // rather than showing an empty list (which is indistinguishable from "no
  // hosts registered" and wastes the operator's triage time).
  const isDisabled = error instanceof ApiError && error.status === 404;
  // Any other fetch error (5xx, network drop) must NOT collapse into
  // `data = undefined` → empty arrays → vacuous `every()` → "all operational".
  // A public status page that lies green during a backend outage is the
  // single worst failure mode for this surface.
  const hasError = Boolean(error) && !isDisabled && !data;

  const hosts: PublicHostStatus[] = data?.hosts ?? [];
  const monitors: PublicMonitorStatus[] = data?.monitors ?? [];

  const allOnline =
    !hasError &&
    hosts.every((h) => h.is_online) &&
    monitors.every((m) => m.is_online);

  const hostRows: StatusRow[] = hosts.map((h) => ({
    key: `host:${h.host_key}`,
    primary: h.display_name,
    secondary: h.host_key,
    is_online: h.is_online,
    uptime: h.uptime_7d,
    uptimeLabel: t.statusPage.uptime7d,
  }));

  const monitorRows: StatusRow[] = monitors.map((m) => ({
    key: `monitor:${m.kind}:${m.monitor_id}`,
    primary: `[${m.kind.toUpperCase()}] ${m.name}`,
    secondary: m.target,
    is_online: m.is_online,
    uptime: m.uptime_24h,
    uptimeLabel: t.statusPage.uptime24h,
  }));

  // Rendering `new Date()` during the render body produces a hydration
  // mismatch on /status (a public SSR-able page) — server wall-clock !==
  // client wall-clock even by a few ms. Keep the timestamp out of the
  // SSR'd HTML until after hydration; `null` on the server, then a real
  // Date on the client. set-state-in-effect is exactly the shape the rule
  // flags, but post-hydration state-seeding is the documented valid escape.
  const [now, setNow] = useState<Date | null>(null);
  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    setNow(new Date());
    const id = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(id);
  }, []);
  /* eslint-enable react-hooks/set-state-in-effect */

  return (
    <div className="page-content fade-in status-page">
      <PageHeader
        icon={<Shield size={16} aria-hidden="true" />}
        title={t.statusPage.title}
        description={t.statusPage.subtitle}
        align="center"
      />

      {isDisabled ? (
        <Panel>
          <EmptyState
            title={t.statusPage.disabledTitle}
            description={t.statusPage.disabledBody}
          />
        </Panel>
      ) : hasError ? (
        <Panel>
          <EmptyState
            tone="error"
            icon={<XCircle size={28} aria-hidden="true" />}
            title={t.statusPage.errorTitle}
            description={t.statusPage.errorBody}
          />
        </Panel>
      ) : (
        <>
          {/* Overall verdict. The banner carries the only saturated
              surface on the page, so "something is wrong" is the one
              thing that reads from across the room. */}
          <div className={`status-banner ${allOnline ? "status-banner--ok" : "status-banner--crit"}`}>
            {allOnline ? (
              <CheckCircle size={18} aria-hidden="true" />
            ) : (
              <XCircle size={18} aria-hidden="true" />
            )}
            <span className="status-banner__text">
              {allOnline ? t.statusPage.allOperational : t.statusPage.someIssues}
            </span>
          </div>

          <StatusSection title={t.statusPage.hostsSection} rows={hostRows} t={t} />
          <StatusSection title={t.statusPage.monitorsSection} rows={monitorRows} t={t} />

          {isLoading && (
            <Panel>
              <div className="skeleton-stack">
                <div className="skeleton" style={{ height: 44 }} />
                <div className="skeleton" style={{ height: 44 }} />
              </div>
            </Panel>
          )}

          {!isLoading && hostRows.length === 0 && monitorRows.length === 0 && (
            <Panel>
              <EmptyState title={t.statusPage.noHosts} />
            </Panel>
          )}
        </>
      )}

      {/* Footer */}
      <div className="status-footer">
        {t.statusPage.lastUpdated}: {now ? now.toLocaleString(locale === "ko" ? "ko-KR" : "en-US") : ""}
      </div>
    </div>
  );
}

/**
 * A section of the public status list. This was previously a plain
 * `renderSection()` function rather than a component, so it could not
 * hold state or hooks and its rows were emitted as bare divs.
 */
function StatusSection({
  title,
  rows,
  t,
}: {
  title: string;
  rows: StatusRow[];
  t: ReturnType<typeof useI18n>["t"];
}) {
  if (rows.length === 0) return null;
  return (
    <section className="status-section">
      <h2 className="status-section__title">{title}</h2>
      <Panel>
        {rows.map((row) => (
          <div key={row.key} className="status-row">
            <StatusDot tone={row.is_online ? "ok" : "crit"} firing={!row.is_online} />

            <div className="status-row__id">
              <span className="status-row__name">{row.primary}</span>
              <span className="status-row__target">{row.secondary}</span>
            </div>

            <Badge tone={row.is_online ? "ok" : "crit"}>
              {row.is_online ? t.statusPage.operational : t.statusPage.down}
            </Badge>

            <div className="status-row__uptime">
              <span
                className="status-row__uptime-value"
                style={{ color: `var(--${uptimeTone(row.uptime)})` }}
              >
                {row.uptime.toFixed(2)}%
              </span>
              <span className="status-row__uptime-label">{row.uptimeLabel}</span>
            </div>
          </div>
        ))}
      </Panel>
    </section>
  );
}
