"use client";

import { useMemo, useState } from "react";
import { Box } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import type { DockerContainer } from "@/app/types/metrics";

interface DockerGridProps {
  containers: DockerContainer[];
}

type FilterMode = "all" | "running" | "attention";

export default function DockerGrid({ containers }: DockerGridProps) {
  const { t } = useI18n();
  const [filter, setFilter] = useState<FilterMode>("all");

  const summary = useMemo(() => {
    let running = 0;
    let attention = 0;
    for (const container of containers) {
      const stopped = container.state !== "running";
      if (stopped) attention += 1;
      else running += 1;
    }
    return { total: containers.length, running, attention };
  }, [containers]);

  const filteredContainers = useMemo(() => {
    switch (filter) {
      case "running":
        return containers.filter((container) => container.state === "running");
      case "attention":
        return containers.filter((container) => container.state !== "running");
      case "all":
        return containers;
    }
  }, [containers, filter]);

  const groups = useMemo(() => {
    const map = new Map<string, DockerContainer[]>();
    for (const container of filteredContainers) {
      const key = container.compose_project || t.dockerGrid.standalone;
      const group = map.get(key);
      if (group) group.push(container);
      else map.set(key, [container]);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [filteredContainers, t.dockerGrid.standalone]);

  if (containers.length === 0) {
    return (
      <div
        style={{
          textAlign: "center",
          padding: "24px 0",
          color: "var(--md-sys-color-on-surface-variant)",
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
      <div className="docker-grid__filters" role="tablist" aria-label={t.host.dockerContainers}>
        <button
          type="button"
          className="docker-grid__filter-btn"
          data-active={filter === "all"}
          aria-pressed={filter === "all"}
          onClick={() => setFilter("all")}
        >
          {t.dockerGrid.total.replace("{count}", String(summary.total))}
        </button>
        <button
          type="button"
          className="docker-grid__filter-btn docker-grid__filter-btn--running"
          data-active={filter === "running"}
          aria-pressed={filter === "running"}
          onClick={() => setFilter("running")}
        >
          <span className="pulse-dot green" style={{ width: 6, height: 6 }} />
          {t.dockerGrid.running.replace("{count}", String(summary.running))}
        </button>
        <button
          type="button"
          className="docker-grid__filter-btn docker-grid__filter-btn--attention"
          data-active={filter === "attention"}
          aria-pressed={filter === "attention"}
          onClick={() => setFilter("attention")}
        >
          {t.dockerGrid.attention.replace("{count}", String(summary.attention))}
        </button>
      </div>

      {filteredContainers.length === 0 && (
        <div className="docker-grid__empty-filter">{t.dockerGrid.noFilterMatches}</div>
      )}

      {groups.map(([groupName, groupContainers]) => (
        <section key={groupName} className="docker-grid__group">
          <div className="docker-grid__group-head">
            <div className="docker-grid__group-meta">
              <div className="docker-grid__group-title">{groupName}</div>
              <div className="docker-grid__group-subtitle">
                {t.dockerGrid.groupTotal.replace("{count}", String(groupContainers.length))}
              </div>
            </div>
            <div className="docker-grid__group-chip">
              {t.dockerGrid.groupRunning
                .replace(
                  "{running}",
                  String(
                    groupContainers.filter((container) => container.state === "running").length,
                  ),
                )
                .replace("{total}", String(groupContainers.length))}
            </div>
          </div>

          <div className="docker-grid__cards">
            {groupContainers.map((container) => {
              const isRunning = container.state === "running";
              const health = container.health_status;
              const tone = container.oom_killed || health === "unhealthy" || !isRunning
                ? "attention"
                : "running";
              const exitCode = container.exit_code;
              const hasExit = exitCode !== null && exitCode !== undefined;
              const hasMetaTags = Boolean(container.compose_service)
                || Boolean(health)
                || container.oom_killed
                || hasExit
                || container.restart_count > 0;

              return (
                <article
                  key={container.container_name}
                  className="docker-grid__card"
                  data-tone={tone}
                >
                  <header className="docker-grid__card-state">
                    <span
                      className={`pulse-dot ${isRunning ? "green" : "red"}`}
                      style={{ width: 6, height: 6 }}
                      aria-hidden="true"
                    />
                    <span className="docker-grid__card-state-text" data-tone={tone}>
                      {container.state}
                    </span>
                  </header>

                  <div className="docker-grid__card-title-wrap">
                    <div className="docker-grid__card-title" title={container.container_name}>
                      {container.container_name}
                    </div>
                    <div className="docker-grid__card-image" title={container.image}>
                      {container.image}
                    </div>
                  </div>

                  <div className="docker-grid__card-status" title={container.status}>
                    {container.status}
                  </div>

                  {hasMetaTags && (
                    <div className="docker-grid__tag-row">
                      {container.compose_service && (
                        <span className="docker-grid__tag">{container.compose_service}</span>
                      )}
                      {health && (
                        <span className="docker-grid__tag" data-tone={health}>
                          {health}
                        </span>
                      )}
                      {container.oom_killed && (
                        <span className="docker-grid__tag" data-tone="attention">
                          OOM
                        </span>
                      )}
                      {hasExit && (
                        <span className="docker-grid__tag">
                          {t.dockerGrid.exitCode.replace("{code}", String(exitCode))}
                        </span>
                      )}
                      {container.restart_count > 0 && (
                        <span className="docker-grid__tag">
                          {t.dockerGrid.restarts.replace(
                            "{count}",
                            String(container.restart_count),
                          )}
                        </span>
                      )}
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}
