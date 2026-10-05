import type {
  DockerContainerStats,
  HostMetricsPayload,
  HostStatusPayload,
} from "@/app/types/metrics";

export function getCurrentDockerStats(
  metrics: HostMetricsPayload | undefined,
  status: HostStatusPayload,
): readonly DockerContainerStats[] {
  return metrics?.docker_stats ?? status.docker_stats;
}
