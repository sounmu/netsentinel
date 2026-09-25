export type PresetKey =
  | "1m"
  | "5m"
  | "1h"
  | "6h"
  | "12h"
  | "24h"
  | "7d"
  | "30d"
  | "custom";

export interface TimeRange {
  start: Date;
  end: Date;
  preset: PresetKey;
}

export function getPresetRangeAt(
  minutes: number,
  preset: PresetKey,
  nowMs: number,
): TimeRange {
  return {
    start: new Date(nowMs - minutes * 60_000),
    end: new Date(nowMs),
    preset,
  };
}

export function isLivePreset(preset: PresetKey): boolean {
  return preset === "1m" || preset === "5m";
}

export function resolveEffectiveRange(
  selectedRange: TimeRange,
  latestLiveTimestamp: string | null,
  initialAnchorTs: number,
): TimeRange {
  if (!isLivePreset(selectedRange.preset)) return selectedRange;

  const parsedLiveTs = latestLiveTimestamp === null
    ? Number.NaN
    : new Date(latestLiveTimestamp).getTime();
  const anchorTs = Number.isFinite(parsedLiveTs) ? parsedLiveTs : initialAnchorTs;
  const windowMs = selectedRange.preset === "1m" ? 60_000 : 300_000;

  return {
    start: new Date(anchorTs - windowMs),
    end: new Date(anchorTs),
    preset: selectedRange.preset,
  };
}

export function formatBrowserTimeZone(date: Date): string {
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absolute = Math.abs(offsetMinutes);
  const hours = String(Math.floor(absolute / 60)).padStart(2, "0");
  const minutes = String(absolute % 60).padStart(2, "0");
  return `UTC${sign}${hours}:${minutes}`;
}

export function customizeDisplayedRange(
  displayedRange: TimeRange,
  boundary: "start" | "end",
  value: Date,
): TimeRange {
  if (boundary === "start") {
    return {
      start: value,
      end: value.getTime() > displayedRange.end.getTime()
        ? value
        : displayedRange.end,
      preset: "custom",
    };
  }
  return {
    start: value.getTime() < displayedRange.start.getTime()
      ? value
      : displayedRange.start,
    end: value,
    preset: "custom",
  };
}
