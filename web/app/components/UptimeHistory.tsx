"use client";

import useSWR from "swr";
import { fetcher, getUptimeUrl, type UptimeSummary } from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";

/** Threshold colors mirror the public status page's `getUptimeColor`. */
function uptimeColor(pct: number): string {
  if (pct >= 99.5) return "var(--accent-green)";
  if (pct >= 95) return "var(--accent-yellow)";
  return "var(--accent-red)";
}

/**
 * Daily uptime breakdown for a host (`GET /api/uptime/:host_key`).
 *
 * Day labels are formatted in the **workspace timezone** reported by the API
 * (`summary.timezone`), not the browser's, so the calendar-day boundaries the
 * bars represent match the server-side grouping. The `day` values are UTC
 * instants of local midnight in that zone.
 */
export default function UptimeHistory({
  hostKey,
  days = 30,
}: {
  hostKey: string;
  days?: number;
}) {
  const { t, locale } = useI18n();
  const { data, isLoading } = useSWR<UptimeSummary>(
    getUptimeUrl(hostKey, days),
    fetcher,
    { refreshInterval: 300_000 },
  );

  if (isLoading && !data) {
    return <div className="skeleton" style={{ height: 64 }} />;
  }
  if (!data || data.daily.length === 0) {
    return <div style={{ fontSize: 13, color: "var(--text-muted)" }}>{t.host.uptimeNoData}</div>;
  }

  // API returns newest-first; show oldest → newest for a left-to-right timeline.
  const points = [...data.daily].reverse();

  const dayFmt = new Intl.DateTimeFormat(locale === "ko" ? "ko-KR" : "en-US", {
    timeZone: data.timezone,
    month: "short",
    day: "numeric",
  });

  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 8,
          flexWrap: "wrap",
          marginBottom: 12,
        }}
      >
        <span style={{ fontSize: 22, fontWeight: 700, color: uptimeColor(data.overall_pct) }}>
          {data.overall_pct.toFixed(2)}%
        </span>
        <span
          style={{
            fontSize: 11,
            color: "var(--text-muted)",
            fontFamily: "var(--font-mono), monospace",
          }}
        >
          {data.timezone}
        </span>
      </div>

      <div style={{ display: "flex", alignItems: "flex-end", gap: 2, height: 48 }}>
        {points.map((p) => (
          <div
            key={p.day}
            title={`${dayFmt.format(new Date(p.day))} — ${p.uptime_pct.toFixed(1)}%`}
            style={{
              flex: 1,
              minWidth: 3,
              height: "100%",
              display: "flex",
              alignItems: "flex-end",
              background: "var(--border-subtle)",
              borderRadius: 2,
              overflow: "hidden",
            }}
          >
            <div
              style={{
                width: "100%",
                height: `${Math.max(p.uptime_pct, 2)}%`,
                background: uptimeColor(p.uptime_pct),
              }}
            />
          </div>
        ))}
      </div>

      <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6 }}>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {dayFmt.format(new Date(points[0].day))}
        </span>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {dayFmt.format(new Date(points[points.length - 1].day))}
        </span>
      </div>
    </div>
  );
}
