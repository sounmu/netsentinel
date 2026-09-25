import { describe, expect, it } from "vitest";
import type { AlertConfigRow } from "@/app/lib/api";
import { resolveGlobalRule, summarizeGlobalRules } from "./rule-summary";

function config(
  metric_type: AlertConfigRow["metric_type"],
  enabled: boolean,
  overrides: Partial<AlertConfigRow> = {},
): AlertConfigRow {
  return {
    id: 1,
    host_key: null,
    metric_type,
    enabled,
    threshold: 80,
    sustained_secs: 300,
    cooldown_secs: 60,
    updated_at: "2026-09-24T00:00:00Z",
    sub_key: null,
    ...overrides,
  };
}

describe("summarizeGlobalRules", () => {
  it("reports built-in CPU, memory, and disk defaults when no rows are saved", () => {
    expect(summarizeGlobalRules([])).toEqual({
      enabledCount: 3,
      totalCount: 4,
      dockerEnabled: false,
      provenance: "builtIn",
    });
    expect(resolveGlobalRule([], "cpu")).toEqual({
      enabled: true,
      threshold: 80,
      source: "builtIn",
    });
    expect(resolveGlobalRule([], "docker")).toEqual({
      enabled: false,
      threshold: 1,
      source: "builtIn",
    });
  });

  it("counts effective top-level defaults and excludes sub-key rows", () => {
    const configs = [
      config("cpu", true),
      config("memory", false),
      config("disk", true),
      config("docker", true),
      config("disk", true, { id: 5, sub_key: "/data" }),
    ];

    expect(summarizeGlobalRules(configs)).toEqual({
      enabledCount: 3,
      totalCount: 4,
      dockerEnabled: true,
      provenance: "saved",
    });
  });

  it("reports fallback use when unrelated saved metrics do not cover the displayed defaults", () => {
    const configs = [
      config("load", true),
      config("network", true),
      config("temperature", true),
      config("gpu", true),
    ];

    expect(summarizeGlobalRules(configs)).toMatchObject({
      enabledCount: 3,
      totalCount: 4,
      dockerEnabled: false,
      provenance: "builtIn",
    });
  });

  it("labels partial saved coverage as mixed rather than wholly built-in", () => {
    expect(summarizeGlobalRules([config("cpu", false)])).toEqual({
      enabledCount: 2,
      totalCount: 4,
      dockerEnabled: false,
      provenance: "mixed",
    });
  });
});
