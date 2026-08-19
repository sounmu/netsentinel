"use client";

import { useEffect } from "react";
import useSWR from "swr";
import Link from "next/link";
import { ArrowRight, ShieldCheck } from "lucide-react";
import {
  AlertHistoryRow,
  getActiveAlertsUrl,
  fetcher,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import { useNowTick } from "@/app/lib/useNowTick";
import { EmptyState, Panel, SkeletonRows } from "@/app/components/ui";
import { alertTypeEmoji, formatRelative, sanitizeMarkdown } from "./shared";

interface Props {
  onCountChange?: (count: number | null) => void;
}

export function ActiveAlertsPanel({ onCountChange }: Props) {
  const { t, locale } = useI18n();
  const { data: active } = useSWR<AlertHistoryRow[]>(
    getActiveAlertsUrl(),
    fetcher,
    { refreshInterval: 15000, revalidateOnFocus: false },
  );

  const nowTick = useNowTick(15_000);

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
      {active === undefined && <SkeletonRows count={2} height={96} />}

      {active && active.length === 0 && (
        <Panel>
          <EmptyState
            icon={<ShieldCheck size={28} aria-hidden="true" />}
            title={t.alerts.active.allClear}
            description={t.alerts.active.allClearDescription}
          />
        </Panel>
      )}

      {active && active.length > 0 && (
        <div className="alerts-active-grid">
          {active.map((alert) => (
            <article
              key={`${alert.host_key}-${alert.alert_type}-${alert.id}`}
              className="alerts-active-card"
            >
              <div className="alerts-active-card__head">
                <div>
                  <div className="alerts-active-card__host">
                    <span aria-hidden="true" className="alerts-active-card__emoji">
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
                  className="btn btn--secondary btn--sm"
                  href={`/host/?key=${encodeURIComponent(alert.host_key)}`}
                  // See `app/page.tsx` for the rationale — static export
                  // with query-string keying makes Next's prefetcher 404
                  // against ServeDir; click-time hydration is unaffected.
                  prefetch={false}
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
