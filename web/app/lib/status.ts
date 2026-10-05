/**
 * Host status determination utility
 *
 * 3-tier determination based on SSE architecture:
 *
 * 1. is_online === false (explicit server down determination) -> offline
 * 2. last_seen timestamp-based safety net (fallback for SSE connection loss, etc.)
 *    - online:  last received <= 1 scrape interval (<= 10s)
 *    - pending: last received 10s ~ 30s (1~3 intervals missed)
 *    - offline: last received > 30s (3+ intervals missed)
 */

export type HostStatus = "online" | "pending" | "offline";

const DEFAULT_SCRAPE_INTERVAL_SEC = 10;

/**
 * Determine host status based on SSE payload.
 *
 * @param lastSeen  - Last received timestamp (ISO string or null)
 * @param isOnline  - Online status determined by the server (scraper) (is_online from SSE payload).
 *                    If undefined, determination is based on timestamp only (backward compatible).
 */
export function getHostStatus(
  lastSeen: string | null,
  isOnline?: boolean,
  scrapeIntervalSec = DEFAULT_SCRAPE_INTERVAL_SEC,
): HostStatus {
  const effectiveInterval = Number.isFinite(scrapeIntervalSec) && scrapeIntervalSec > 0
    ? scrapeIntervalSec
    : DEFAULT_SCRAPE_INTERVAL_SEC;
  const pendingThresholdSec = effectiveInterval;
  const offlineThresholdSec = effectiveInterval * 3;

  // Server explicitly determined as down — takes priority over timestamp calculation
  // However, if last_seen is absent, it has never been scraped, so return "pending" (status unknown)
  if (isOnline === false) {
    return lastSeen ? "offline" : "pending";
  }

  // No timestamp means unobserved state -> pending (was previously offline, but semantically inaccurate)
  if (!lastSeen) return "pending";

  const diffSec = (Date.now() - new Date(lastSeen).getTime()) / 1000;

  if (diffSec <= pendingThresholdSec) return "online";
  if (diffSec <= offlineThresholdSec) return "pending";
  return "offline";
}

/** Colors per status */
export const STATUS_COLORS: Record<HostStatus, { accent: string; bg: string; border: string }> = {
  online:  { accent: "var(--ok)",   bg: "var(--ok-bg)",   border: "var(--badge-online-border)" },
  pending: { accent: "var(--warn)", bg: "var(--warn-bg)", border: "var(--badge-pending-border)" },
  offline: { accent: "var(--crit)", bg: "var(--crit-bg)", border: "var(--badge-offline-border)" },
};

/**
 * Single source of truth for "how bad is this percentage".
 *
 * Every surface that draws a utilisation meter — the overview table,
 * the containers table, host detail — reads its colour from here.
 * These thresholds used to be duplicated per page (and the overview
 * ignored them entirely, painting every meter green regardless of
 * value), so the same 88% reading could be described three different
 * ways depending on which route you were looking at.
 */
export type MeterTone = "ok" | "warn" | "crit";

export const METER_WARN_PCT = 60;
export const METER_CRIT_PCT = 85;

export function meterTone(percent: number): MeterTone {
  if (percent >= METER_CRIT_PCT) return "crit";
  if (percent >= METER_WARN_PCT) return "warn";
  return "ok";
}

/** CSS custom property holding the colour for a meter tone. */
export const METER_TONE_VAR: Record<MeterTone, string> = {
  ok:   "var(--ok)",
  warn: "var(--warn)",
  crit: "var(--crit)",
};

/**
 * Uptime percentages run the other way round — higher is better —
 * and were previously thresholded at 99/95 on /monitors but 99.5/95
 * on /status, so a monitor at 99.2% showed green on one page and
 * amber on the other.
 */
export function uptimeTone(percent: number): MeterTone {
  if (percent >= 99.5) return "ok";
  if (percent >= 95) return "warn";
  return "crit";
}

/** Labels per status */
export const STATUS_LABELS: Record<HostStatus, string> = {
  online:  "Online",
  pending: "Pending",
  offline: "Offline",
};

/** Badge CSS class per status */
export const STATUS_BADGE_CLASS: Record<HostStatus, string> = {
  online:  "badge-online",
  pending: "badge-pending",
  offline: "badge-offline",
};

/** Pulse dot CSS class per status */
export const STATUS_DOT_CLASS: Record<HostStatus, string> = {
  online:  "pulse-dot green",
  pending: "pulse-dot yellow",
  offline: "pulse-dot red",
};

/** Sub-labels per status (for Sidebar, server list) */
export const STATUS_SUB_LABELS: Record<HostStatus, string> = {
  online:  "Active Node",
  pending: "Reconnecting...",
  offline: "Offline Node",
};
