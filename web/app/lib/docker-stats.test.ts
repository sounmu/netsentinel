import { describe, expect, it } from "vitest";
import type {
  DockerContainerStats,
  HostMetricsPayload,
  HostStatusPayload,
} from "@/app/types/metrics";
import { getCurrentDockerStats } from "./docker-stats";

const staleStat: DockerContainerStats = {
  container_name: "api",
  cpu_percent: 1,
  memory_usage_mb: 10,
  memory_limit_mb: 100,
  net_rx_bytes: 1,
  net_tx_bytes: 2,
  block_read_bytes: 3,
  block_write_bytes: 4,
};

const liveStat: DockerContainerStats = {
  ...staleStat,
  cpu_percent: 9,
};

function status(): HostStatusPayload {
  return {
    host_key: "box:9101",
    display_name: "box",
    scrape_interval_secs: 10,
    is_online: true,
    last_seen: "2026-09-24T00:00:00Z",
    docker_containers: [],
    ports: [],
    disks: [],
    processes: [],
    temperatures: [],
    gpus: [],
    docker_stats: [staleStat],
  };
}

function metrics(): HostMetricsPayload {
  return {
    host_key: "box:9101",
    display_name: "box",
    is_online: true,
    cpu_usage_percent: 1,
    memory_usage_percent: 2,
    load_1min: 0.1,
    load_5min: 0.2,
    load_15min: 0.3,
    network_rate: {
      rx_bytes_per_sec: 1,
      tx_bytes_per_sec: 2,
      total_rx_bytes: 3,
      total_tx_bytes: 4,
    },
    cpu_cores: [],
    network_interface_rates: [],
    disks: [],
    temperatures: [],
    docker_stats: [liveStat],
    timestamp: "2026-09-24T00:00:10Z",
  };
}

describe("getCurrentDockerStats", () => {
  it("uses the every-scrape metrics event instead of a stale status snapshot", () => {
    expect(getCurrentDockerStats(metrics(), status())).toEqual([liveStat]);
  });

  it("uses the status snapshot before the first metrics event arrives", () => {
    expect(getCurrentDockerStats(undefined, status())).toEqual([staleStat]);
  });
});
