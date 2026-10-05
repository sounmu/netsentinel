import type { DockerContainer } from "@/app/types/metrics";

export type ContainerOperationalCategory =
  | "attention"
  | "running"
  | "inactive"
  | "completed";

export type ContainerAttentionReason =
  | "oom"
  | "unhealthy"
  | "restarting"
  | "dead"
  | "failedExit";

export type ContainerInactiveReason =
  | "created"
  | "paused"
  | "exitedUnknown"
  | "other";

export interface ContainerAssessment {
  category: ContainerOperationalCategory;
  attentionReason: ContainerAttentionReason | null;
  inactiveReason: ContainerInactiveReason | null;
}

export interface ContainerOperationRecord {
  key: string;
  hostKey: string;
  hostDisplayName: string;
  container: DockerContainer;
  assessment: ContainerAssessment;
}

export interface ContainerSummary {
  total: number;
  running: number;
  attention: number;
  inactive: number;
  completed: number;
}

export type ContainerGroupMode = "none" | "stack" | "service";

export interface ContainerGroup<T extends ContainerOperationRecord> {
  key: string;
  rows: readonly T[];
}

export function assessContainer(container: DockerContainer): ContainerAssessment {
  const state = container.state.toLowerCase();
  const health = container.health_status?.toLowerCase() ?? null;

  if (container.oom_killed) {
    return { category: "attention", attentionReason: "oom", inactiveReason: null };
  }
  if (health === "unhealthy") {
    return { category: "attention", attentionReason: "unhealthy", inactiveReason: null };
  }
  if (state === "restarting") {
    return { category: "attention", attentionReason: "restarting", inactiveReason: null };
  }
  if (state === "dead") {
    return { category: "attention", attentionReason: "dead", inactiveReason: null };
  }
  if (state === "exited" && container.exit_code !== null && container.exit_code !== undefined) {
    if (container.exit_code !== 0) {
      return { category: "attention", attentionReason: "failedExit", inactiveReason: null };
    }
    return { category: "completed", attentionReason: null, inactiveReason: null };
  }
  if (state === "running") {
    return { category: "running", attentionReason: null, inactiveReason: null };
  }
  if (state === "created") {
    return { category: "inactive", attentionReason: null, inactiveReason: "created" };
  }
  if (state === "paused") {
    return { category: "inactive", attentionReason: null, inactiveReason: "paused" };
  }
  if (state === "exited") {
    return { category: "inactive", attentionReason: null, inactiveReason: "exitedUnknown" };
  }
  return { category: "inactive", attentionReason: null, inactiveReason: "other" };
}

export function summarizeContainers(
  containers: readonly DockerContainer[],
): ContainerSummary {
  const summary: ContainerSummary = {
    total: containers.length,
    running: 0,
    attention: 0,
    inactive: 0,
    completed: 0,
  };

  for (const container of containers) {
    summary[assessContainer(container).category] += 1;
  }
  return summary;
}

export function operationalCategoryRank(
  category: ContainerOperationalCategory,
): number {
  switch (category) {
    case "attention":
      return 0;
    case "running":
      return 1;
    case "inactive":
      return 2;
    case "completed":
      return 3;
  }
}

export function matchesContainerQuery(
  record: ContainerOperationRecord,
  query: string,
): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return true;

  const { container } = record;
  return [
    container.container_name,
    container.image,
    container.state,
    container.status,
    container.compose_project,
    container.compose_service,
    record.hostDisplayName,
    record.hostKey,
  ].some((value) => value?.toLocaleLowerCase().includes(normalized));
}

export function filterContainerRecords<T extends ContainerOperationRecord>(
  records: readonly T[],
  options: {
    query: string;
    category: ContainerOperationalCategory | "all";
    observedState: string | "all";
  },
): readonly T[] {
  return records.filter((record) => {
    if (!matchesContainerQuery(record, options.query)) return false;
    if (options.category !== "all" && record.assessment.category !== options.category) {
      return false;
    }
    return options.observedState === "all"
      || record.container.state.toLowerCase() === options.observedState.toLowerCase();
  });
}

export function groupContainerRecords<T extends ContainerOperationRecord>(
  records: readonly T[],
  mode: ContainerGroupMode,
): readonly ContainerGroup<T>[] {
  if (mode === "none") {
    return [{ key: "all", rows: records }];
  }

  const groups = new Map<string, T[]>();
  for (const record of records) {
    const project = record.container.compose_project?.trim() || "standalone";
    const service = record.container.compose_service?.trim() || "unlabelled";
    const key = mode === "stack" ? project : `${project} / ${service}`;
    const group = groups.get(key);
    if (group) {
      group.push(record);
    } else {
      groups.set(key, [record]);
    }
  }

  return [...groups.entries()]
    .map(([key, rows]) => ({ key, rows }))
    .sort((a, b) => {
      const aRank = Math.min(...a.rows.map((row) => operationalCategoryRank(row.assessment.category)));
      const bRank = Math.min(...b.rows.map((row) => operationalCategoryRank(row.assessment.category)));
      return aRank - bRank || a.key.localeCompare(b.key, undefined, {
        numeric: true,
        sensitivity: "base",
      });
    });
}
