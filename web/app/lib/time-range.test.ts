import { describe, expect, it } from "vitest";
import {
  customizeDisplayedRange,
  formatBrowserTimeZone,
  getPresetRangeAt,
  resolveEffectiveRange,
} from "./time-range";

describe("resolveEffectiveRange", () => {
  it("anchors live presets to the newest sample while preserving the exact window", () => {
    const selected = getPresetRangeAt(5, "5m", Date.parse("2026-09-24T05:00:00Z"));
    const effective = resolveEffectiveRange(
      selected,
      "2026-09-24T05:11:00Z",
      Date.parse("2026-09-24T05:00:00Z"),
    );

    expect(effective.start.toISOString()).toBe("2026-09-24T05:06:00.000Z");
    expect(effective.end.toISOString()).toBe("2026-09-24T05:11:00.000Z");
    expect(effective.end.getTime() - effective.start.getTime()).toBe(300_000);
  });

  it("keeps custom ranges unchanged and falls back deterministically before live data", () => {
    const custom = {
      start: new Date("2026-09-24T04:00:00Z"),
      end: new Date("2026-09-24T05:00:00Z"),
      preset: "custom" as const,
    };
    expect(resolveEffectiveRange(custom, "2026-09-24T06:00:00Z", 0)).toBe(custom);

    const live = getPresetRangeAt(1, "1m", 0);
    const effective = resolveEffectiveRange(live, "invalid", 120_000);
    expect(effective.start.getTime()).toBe(60_000);
    expect(effective.end.getTime()).toBe(120_000);
  });
});

describe("formatBrowserTimeZone", () => {
  it("returns an explicit UTC offset label", () => {
    expect(formatBrowserTimeZone(new Date()).startsWith("UTC")).toBe(true);
  });
});

describe("customizeDisplayedRange", () => {
  const displayed = resolveEffectiveRange(
    getPresetRangeAt(5, "5m", Date.parse("2026-09-24T05:00:00Z")),
    "2026-09-24T05:11:00Z",
    Date.parse("2026-09-24T05:00:00Z"),
  );

  it("uses the current live window's opposite endpoint instead of the stale selected range", () => {
    const next = customizeDisplayedRange(
      displayed,
      "start",
      new Date("2026-09-24T05:08:00Z"),
    );

    expect(next.start.toISOString()).toBe("2026-09-24T05:08:00.000Z");
    expect(next.end.toISOString()).toBe("2026-09-24T05:11:00.000Z");
    expect(next.preset).toBe("custom");
  });

  it("prevents reversed custom intervals in either edit direction", () => {
    const startPastEnd = customizeDisplayedRange(
      displayed,
      "start",
      new Date("2026-09-24T05:12:00Z"),
    );
    expect(startPastEnd.start.getTime()).toBe(startPastEnd.end.getTime());

    const endBeforeStart = customizeDisplayedRange(
      displayed,
      "end",
      new Date("2026-09-24T05:05:00Z"),
    );
    expect(endBeforeStart.start.getTime()).toBe(endBeforeStart.end.getTime());
  });
});
