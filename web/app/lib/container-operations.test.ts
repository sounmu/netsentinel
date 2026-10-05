import { describe, expect, it } from "vitest";
import type { DockerContainer } from "@/app/types/metrics";
import {
  assessContainer,
  filterContainerRecords,
  groupContainerRecords,
  operationalCategoryRank,
  summarizeContainers,
  type ContainerOperationRecord,
} from "./container-operations";

function container(
  state: string,
  overrides: Partial<DockerContainer> = {},
): DockerContainer {
  return {
    container_name: `${state}-${overrides.exit_code ?? "none"}`,
    image: "example:latest",
    state,
    status: state,
    oom_killed: false,
    exit_code: null,
    restart_count: 0,
    compose_project: null,
    compose_service: null,
    health_status: null,
    ...overrides,
  };
}

function record(
  item: DockerContainer,
  overrides: Partial<ContainerOperationRecord> = {},
): ContainerOperationRecord {
  return {
    key: item.container_name,
    hostKey: "box:9101",
    hostDisplayName: "box",
    container: item,
    assessment: assessContainer(item),
    ...overrides,
  };
}

describe("assessContainer", () => {
  it("separates actionable failures from successful and ambiguous stopped states", () => {
    expect(assessContainer(container("running")).category).toBe("running");
    expect(assessContainer(container("created"))).toEqual({
      category: "inactive",
      attentionReason: null,
      inactiveReason: "created",
    });
    expect(assessContainer(container("exited", { exit_code: 0 })).category).toBe("completed");
    expect(assessContainer(container("exited", { exit_code: 2 }))).toMatchObject({
      category: "attention",
      attentionReason: "failedExit",
    });
    expect(assessContainer(container("running", { health_status: "unhealthy" }))).toMatchObject({
      category: "attention",
      attentionReason: "unhealthy",
    });
    expect(assessContainer(container("exited", { exit_code: 0, oom_killed: true }))).toMatchObject({
      category: "attention",
      attentionReason: "oom",
    });
    expect(assessContainer(container("restarting")).attentionReason).toBe("restarting");
    expect(assessContainer(container("dead")).attentionReason).toBe("dead");
  });

  it("summarizes five running containers without treating created and exit-zero jobs as failures", () => {
    const containers = [
      ...Array.from({ length: 5 }, (_, index) => container("running", {
        container_name: `running-${index}`,
      })),
      ...Array.from({ length: 18 }, (_, index) => container("created", {
        container_name: `created-${index}`,
      })),
      ...Array.from({ length: 4 }, (_, index) => container("exited", {
        container_name: `migrate-${index}`,
        exit_code: 0,
      })),
      container("exited", { container_name: "failed", exit_code: 1 }),
      container("running", { container_name: "unhealthy", health_status: "unhealthy" }),
    ];

    expect(summarizeContainers(containers)).toEqual({
      total: 29,
      running: 5,
      attention: 2,
      inactive: 18,
      completed: 4,
    });
  });
});

describe("container operations", () => {
  const rows = [
    record(container("running", {
      container_name: "api",
      compose_project: "platform",
      compose_service: "api",
    })),
    record(container("created", {
      container_name: "old-web",
      compose_project: "legacy",
      compose_service: "web",
    })),
    record(container("exited", {
      container_name: "migrate",
      compose_project: "platform",
      compose_service: "migrate",
      exit_code: 0,
    })),
    record(container("dead", {
      container_name: "worker",
      compose_project: "platform",
      compose_service: "worker",
    })),
  ];

  it("filters by search, operational category, and observed Docker state", () => {
    expect(filterContainerRecords(rows, {
      query: "legacy",
      category: "all",
      observedState: "all",
    }).map((row) => row.key)).toEqual(["old-web"]);

    expect(filterContainerRecords(rows, {
      query: "",
      category: "completed",
      observedState: "exited",
    }).map((row) => row.key)).toEqual(["migrate"]);
  });

  it("groups by stack or service and prioritizes groups containing actionable rows", () => {
    expect(groupContainerRecords(rows, "stack").map((group) => group.key)).toEqual([
      "platform",
      "legacy",
    ]);
    expect(groupContainerRecords(rows, "service").map((group) => group.key)).toEqual([
      "platform / worker",
      "platform / api",
      "legacy / web",
      "platform / migrate",
    ]);
    expect([
      "attention",
      "running",
      "inactive",
      "completed",
    ].map((category) => operationalCategoryRank(category as Parameters<typeof operationalCategoryRank>[0])))
      .toEqual([0, 1, 2, 3]);
  });
});
