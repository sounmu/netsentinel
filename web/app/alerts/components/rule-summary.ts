import type { AlertConfigRow } from "@/app/lib/api";

export interface GlobalRuleSummary {
  enabledCount: number;
  totalCount: number;
  dockerEnabled: boolean;
  provenance: "builtIn" | "saved" | "mixed";
}

export type GlobalRuleMetric = "cpu" | "memory" | "disk" | "docker";

export interface ResolvedGlobalRule {
  enabled: boolean;
  threshold: number;
  source: "saved" | "builtIn";
}

const DEFAULT_RULES = [
  { metric: "cpu", enabled: true, threshold: 80 },
  { metric: "memory", enabled: true, threshold: 90 },
  { metric: "disk", enabled: true, threshold: 90 },
  { metric: "docker", enabled: false, threshold: 1 },
] as const satisfies readonly {
  metric: GlobalRuleMetric;
  enabled: boolean;
  threshold: number;
}[];

export function resolveGlobalRule(
  configs: AlertConfigRow[],
  metric: GlobalRuleMetric,
): ResolvedGlobalRule {
  const saved = configs.find(
    (config) => config.metric_type === metric && !config.sub_key,
  );
  if (saved) {
    return {
      enabled: saved.enabled,
      threshold: saved.threshold,
      source: "saved",
    };
  }

  const builtIn = DEFAULT_RULES.find((rule) => rule.metric === metric);
  if (!builtIn) {
    throw new Error(`Missing built-in alert rule for ${metric}`);
  }
  return {
    enabled: builtIn.enabled,
    threshold: builtIn.threshold,
    source: "builtIn",
  };
}

export function summarizeGlobalRules(configs: AlertConfigRow[]): GlobalRuleSummary {
  const resolved = DEFAULT_RULES.map(
    ({ metric }) => resolveGlobalRule(configs, metric),
  );
  return {
    enabledCount: resolved.filter((rule) => rule.enabled).length,
    totalCount: resolved.length,
    dockerEnabled: resolveGlobalRule(configs, "docker").enabled,
    provenance: resolved.every((rule) => rule.source === "builtIn")
      ? "builtIn"
      : resolved.every((rule) => rule.source === "saved")
        ? "saved"
        : "mixed",
  };
}
