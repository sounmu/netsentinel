"use client";

import { useEffect, useMemo, useSyncExternalStore } from "react";
import useSWR from "swr";
import Link from "next/link";
import { ArrowRight, ShieldCheck } from "lucide-react";
import {
  AlertHistoryRow,
  getAlertHistoryUrl,
  fetcher,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import { alertTypeEmoji, formatRelative, sanitizeMarkdown } from "./shared";

interface ActiveAlert {
  host_key: string;
  alert_type: string;
  message: string;
  created_at: string;
}

/**
 * Client-side computation: an alert is "active" when the latest event for
 * (host_key, base_type) is an overload/down and not followed by a recovery.
 * Phase 3 will replace this with GET /api/alerts/active.
 */
function computeActive(history: AlertHistoryRow[]): ActiveAlert[] {
  const latest = new Map<string, AlertHistoryRow>();
  const sorted = [...history].sort(
    (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime(),
  );
  for (const row of sorted) {
    const key = `${row.host_key}::${baseKind(row.alert_type)}`;
    latest.set(key, row);
  }
  return Array.from(latest.values())
    .filter((row) => isFiring(row.alert_type))
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
}

function baseKind(alertType: string): string {
  return alertType.replace(/_(overload|recovery|down)$/, "");
}

function isFiring(alertType: string): boolean {
  return alertType.endsWith("_overload") || alertType.endsWith("_down");
}

interface Props {
  onCountChange?: (count: number | null) => void;
}

export function ActiveAlertsPanel({ onCountChange }: Props) {
  const { t, locale } = useI18n();
  const { data } = useSWR<AlertHistoryRow[]>(
    getAlertHistoryUrl(undefined, 200),
    fetcher,
    { refreshInterval: 15000, revalidateOnFocus: false },
  );

  const nowTick = useSyncExternalStore(
    (onChange) => {
      const id = setInterval(onChange, 15000);
      return () => clearInterval(id);
    },
    () => Date.now(),
    () => 0,
  );

  const active = useMemo(() => (data ? computeActive(data) : null), [data]);

  useEffect(() => {
    onCountChange?.(active?.length ?? null);
  }, [active, onCountChange]);

  return (
    <div
      className="alerts-panel"
      id="alerts-panel-active"
      role="tabpanel"
      aria-labelledby="alerts-tab-active"
    >
      {active === null && <div className="skeleton" style={{ height: 220 }} />}

      {active && active.length === 0 && (
        <div className="alerts-card alerts-empty" role="status">
          <span className="alerts-empty__icon" aria-hidden="true">
            <ShieldCheck size={28} />
          </span>
          <span className="alerts-empty__title">{t.alerts.active.allClear}</span>
          <span className="alerts-empty__description">{t.alerts.active.allClearDescription}</span>
        </div>
      )}

      {active && active.length > 0 && (
        <div className="alerts-active-grid">
          {active.map((alert) => (
            <article
              key={`${alert.host_key}-${alert.alert_type}-${alert.created_at}`}
              className="alerts-active-card"
            >
              <div className="alerts-active-card__head">
                <div>
                  <div className="alerts-active-card__host">
                    <span aria-hidden="true" style={{ marginRight: 6 }}>
                      {alertTypeEmoji(alert.alert_type)}
                    </span>
                    {alert.host_key}
                  </div>
                  <div className="alerts-active-card__key">{alert.alert_type}</div>
                </div>
                <span className="alerts-active-card__since">
                  {formatRelative(
                    alert.created_at,
                    locale,
                    nowTick || Date.parse(alert.created_at),
                  )}
                </span>
              </div>
              <span className="alerts-severity alerts-severity--critical">
                {t.alerts.summary.active}
              </span>
              <p className="alerts-active-card__message">{sanitizeMarkdown(alert.message)}</p>
              <div className="alerts-active-card__actions">
                <Link
                  className="alerts-btn alerts-btn--sm alerts-btn--tonal"
                  href={`/host/${encodeURIComponent(alert.host_key)}`}
                >
                  <ArrowRight size={12} aria-hidden="true" />
                  {t.alerts.active.viewHost}
                </Link>
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
