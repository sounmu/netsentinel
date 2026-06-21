"use client";

import useSWR from "swr";
import {
  fetcher,
  getUptimeUrl,
  type UptimePoint,
  type UptimeSummary,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";

const DEFAULT_UPTIME_DAYS = 31;

function dateKeyInTimeZone(date: Date, timeZone: string): string {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(date);
  const part = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${part("year")}-${part("month")}-${part("day")}`;
}

function shiftDateKey(dateKey: string, days: number): string {
  const [year, month, day] = dateKey.split("-").map(Number);
  return new Date(Date.UTC(year, month - 1, day + days)).toISOString().slice(0, 10);
}

function buildFixedDateKeys(days: number, timeZone: string): string[] {
  const todayKey = dateKeyInTimeZone(new Date(), timeZone);
  return Array.from({ length: days }, (_, index) =>
    shiftDateKey(todayKey, index - days + 1),
  );
}

function formatDateKey(dateKey: string, locale: string): string {
  const [year, month, day] = dateKey.split("-").map(Number);
  return new Intl.DateTimeFormat(locale, {
    timeZone: "UTC",
    month: "short",
    day: "numeric",
  }).format(new Date(Date.UTC(year, month - 1, day)));
}

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
  days = DEFAULT_UPTIME_DAYS,
}: {
  hostKey: string;
  days?: number;
}) {
  const { t, locale } = useI18n();
  const displayDays = Math.max(days, 1);
  const { data, isLoading } = useSWR<UptimeSummary>(
    getUptimeUrl(hostKey, displayDays),
    fetcher,
    { refreshInterval: 300_000 },
  );

  if (isLoading && !data) {
    return <div className="skeleton" style={{ height: 64 }} />;
  }
  if (!data) {
    return <div style={{ fontSize: 13, color: "var(--text-muted)" }}>{t.host.uptimeNoData}</div>;
  }

  const loc = locale === "ko" ? "ko-KR" : "en-US";
  const pointsByDay = new Map<string, UptimePoint>();
  for (const point of data.daily) {
    pointsByDay.set(dateKeyInTimeZone(new Date(point.day), data.timezone), point);
  }

  const axis = buildFixedDateKeys(displayDays, data.timezone).map((dayKey) => ({
    dayKey,
    point: pointsByDay.get(dayKey),
  }));
  const hasSamples = data.daily.some((point) => point.total_count > 0);

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
        <span
          style={{
            fontSize: 22,
            fontWeight: 700,
            color: hasSamples ? uptimeColor(data.overall_pct) : "var(--text-muted)",
          }}
        >
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
        {axis.map(({ dayKey, point }) => {
          const label = formatDateKey(dayKey, loc);
          return (
            <div
              key={dayKey}
              title={
                point
                  ? `${label} - ${point.uptime_pct.toFixed(1)}%`
                  : `${label} - ${t.chart.noData}`
              }
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
              {point && (
                <div
                  style={{
                    width: "100%",
                    height: `${Math.max(point.uptime_pct, 2)}%`,
                    background: uptimeColor(point.uptime_pct),
                  }}
                />
              )}
            </div>
          );
        })}
      </div>

      <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6 }}>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {formatDateKey(axis[0].dayKey, loc)}
        </span>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
          {formatDateKey(axis[axis.length - 1].dayKey, loc)}
        </span>
      </div>
    </div>
  );
}
