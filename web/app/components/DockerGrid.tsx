"use client";

import { DockerContainer, DockerContainerStats } from "@/app/types/metrics";
import { useMemo } from "react";
import { Box, Cpu } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import { formatBytes } from "@/app/lib/formatters";

interface DockerGridProps {
  containers: DockerContainer[];
  stats?: DockerContainerStats[];
}

export default function DockerGrid({ containers, stats }: DockerGridProps) {
  const { t } = useI18n();
  const statsByName = useMemo(() => {
    const map = new Map<string, DockerContainerStats>();
    for (const stat of stats ?? []) map.set(stat.container_name, stat);
    return map;
  }, [stats]);
  const groups = useMemo(() => {
    const map = new Map<string, DockerContainer[]>();
    for (const container of containers) {
      const key = container.compose_project || t.dockerGrid.standalone;
      const group = map.get(key);
      if (group) group.push(container);
      else map.set(key, [container]);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [containers, t.dockerGrid.standalone]);
  const summary = useMemo(() => {
    let running = 0;
    let attention = 0;
    for (const container of containers) {
      const unhealthy = container.health_status === "unhealthy";
      const stopped = container.state !== "running";
      if (!stopped) running += 1;
      if (stopped || unhealthy || container.oom_killed) attention += 1;
    }
    return { total: containers.length, running, attention };
  }, [containers]);

  if (containers.length === 0) {
    return (
      <div
        style={{
          textAlign: "center",
          padding: "24px 0",
          color: "var(--text-muted)",
          fontSize: 13,
        }}
      >
        <Box size={28} style={{ margin: "0 auto 8px", opacity: 0.4 }} />
        <div>{t.dockerGrid.noContainers}</div>
      </div>
    );
  }

  return (
    <div style={{ display: "grid", gap: 16 }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 8,
          fontSize: 11,
          color: "var(--text-muted)",
        }}
      >
        <span>{t.dockerGrid.total.replace("{count}", String(summary.total))}</span>
        <span>{t.dockerGrid.running.replace("{count}", String(summary.running))}</span>
        <span
          style={{
            color: summary.attention > 0 ? "var(--accent-red)" : "var(--text-muted)",
            fontWeight: summary.attention > 0 ? 700 : 500,
          }}
        >
          {t.dockerGrid.attention.replace("{count}", String(summary.attention))}
        </span>
      </div>
      {groups.map(([groupName, groupContainers]) => (
        <section key={groupName} style={{ display: "grid", gap: 8 }}>
          <div
            style={{
              fontSize: 12,
              fontWeight: 700,
              color: "var(--text-secondary)",
            }}
          >
            {groupName}
          </div>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
              gap: 10,
            }}
          >
      {groupContainers.map((c) => {
        const isRunning = c.state === "running";
        const stat = statsByName.get(c.container_name);
        const health = c.health_status;
        const showLifecycle = c.oom_killed || c.exit_code !== null && c.exit_code !== undefined || c.restart_count > 0 || health;
        return (
          <div
            key={c.container_name}
            style={{
              background: isRunning ? "var(--status-online-bg)" : "var(--status-offline-bg)",
              border: `1px solid ${isRunning ? "var(--badge-online-border)" : "var(--badge-offline-border)"}`,
              borderRadius: 8,
              padding: "12px 14px",
              transition: "all 0.2s",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 10,
                marginBottom: 8,
              }}
            >
              <div
                style={{
                  width: 32,
                  height: 32,
                  borderRadius: 8,
                  background: isRunning ? "var(--status-online-bg-light)" : "var(--status-offline-bg-light)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                }}
              >
                <Cpu
                  size={16}
                  color={isRunning ? "var(--accent-green)" : "var(--accent-red)"}
                />
              </div>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div
                  style={{
                    fontSize: 13,
                    fontWeight: 700,
                    color: "var(--text-primary)",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                  title={c.container_name}
                >
                  {c.container_name}
                </div>
                <div
                  style={{
                    fontSize: 11,
                    color: "var(--text-muted)",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    fontFamily: "monospace",
                  }}
                  title={c.image}
                >
                  {c.image}
                </div>
              </div>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 4,
                  padding: "2px 8px",
                  borderRadius: 6,
                  fontSize: 10,
                  fontWeight: 700,
                  letterSpacing: "0.5px",
                  textTransform: "uppercase",
                  background: isRunning ? "var(--status-online-bg-light)" : "var(--status-offline-bg-light)",
                  color: isRunning ? "var(--badge-online-text)" : "var(--badge-offline-text)",
                }}
              >
                <span
                  className={`pulse-dot ${isRunning ? "green" : "red"}`}
                  style={{ width: 5, height: 5 }}
                />
                {c.state}
              </span>
              <span
                style={{
                  fontSize: 11,
                  color: "var(--text-muted)",
                  flex: 1,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={c.status}
              >
                {c.status}
              </span>
            </div>
            {showLifecycle && (
              <div
                style={{
                  marginTop: 8,
                  display: "flex",
                  flexWrap: "wrap",
                  gap: 6,
                  fontSize: 10,
                  color: "var(--text-muted)",
                }}
              >
                {health && (
                  <span title="health">{health}</span>
                )}
                {c.exit_code !== null && c.exit_code !== undefined && (
                  <span>exit {c.exit_code}</span>
                )}
                {c.oom_killed && (
                  <span style={{ color: "var(--accent-red)", fontWeight: 700 }}>OOM</span>
                )}
                {c.restart_count > 0 && (
                  <span>restarts {c.restart_count}</span>
                )}
                {c.compose_service && (
                  <span>{c.compose_service}</span>
                )}
              </div>
            )}
            {stat && isRunning && (
              <div
                style={{
                  marginTop: 8,
                  paddingTop: 8,
                  borderTop: `1px solid ${isRunning ? "var(--badge-online-border)" : "var(--border-color)"}`,
                  display: "flex",
                  flexWrap: "wrap",
                  gap: "4px 12px",
                  fontSize: 11,
                  color: "var(--text-muted)",
                  fontFamily: "var(--font-mono), monospace",
                }}
              >
                <span>
                  CPU{" "}
                  <span style={{ color: stat.cpu_percent > 80 ? "var(--accent-red)" : "var(--text-primary)", fontWeight: 600 }}>
                    {stat.cpu_percent.toFixed(1)}%
                  </span>
                </span>
                <span>
                  MEM{" "}
                  <span style={{ color: "var(--text-primary)", fontWeight: 600 }}>
                    {stat.memory_usage_mb}/{stat.memory_limit_mb}MB
                  </span>
                </span>
                <span>
                  NET{" "}
                  <span style={{ color: "var(--accent-green)" }}>
                    {formatBytes(stat.net_rx_bytes)}
                  </span>
                  {" / "}
                  <span style={{ color: "var(--accent-blue)" }}>
                    {formatBytes(stat.net_tx_bytes)}
                  </span>
                </span>
                {(stat.block_read_bytes > 0 || stat.block_write_bytes > 0) && (
                  <span>
                    IO{" "}
                    <span style={{ color: "var(--accent-green)" }}>
                      {formatBytes(stat.block_read_bytes ?? 0)}
                    </span>
                    {" / "}
                    <span style={{ color: "var(--accent-blue)" }}>
                      {formatBytes(stat.block_write_bytes ?? 0)}
                    </span>
                  </span>
                )}
              </div>
            )}
          </div>
        );
      })}
          </div>
        </section>
      ))}
    </div>
  );
}
