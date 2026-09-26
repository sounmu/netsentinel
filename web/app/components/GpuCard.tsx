"use client";

import type { GpuInfo } from "../types/metrics";
import { useI18n } from "../i18n/I18nContext";

interface GpuCardProps {
  gpus: GpuInfo[];
}

function formatMemory(mb: number): string {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(0)} MB`;
}

export default function GpuCard({ gpus }: GpuCardProps) {
  const { t } = useI18n();

  return (
    <div style={{ display: "grid", gap: 16 }}>
      {gpus.map((gpu, index) => {
        const memoryPercent = gpu.memory_used_mb != null && gpu.memory_total_mb != null && gpu.memory_total_mb > 0
          ? (gpu.memory_used_mb / gpu.memory_total_mb) * 100
          : null;
        const metrics = [
          [t.gpu.usage, gpu.gpu_usage_percent != null ? `${gpu.gpu_usage_percent.toFixed(1)}%` : "—"],
          [t.gpu.memory, `${gpu.memory_used_mb != null ? formatMemory(gpu.memory_used_mb) : "—"} / ${gpu.memory_total_mb != null && gpu.memory_total_mb > 0 ? formatMemory(gpu.memory_total_mb) : "—"} (${memoryPercent != null ? `${memoryPercent.toFixed(1)}%` : "—"})`],
          [t.gpu.temperature, gpu.temperature_c != null ? `${gpu.temperature_c.toFixed(1)}°C` : "—"],
          [t.gpu.power, `${gpu.power_watts != null ? `${gpu.power_watts.toFixed(1)} W` : "—"} / ${gpu.power_limit_watts != null ? `${gpu.power_limit_watts.toFixed(1)} W` : "—"}`],
        ];
        return (
          <div key={`${gpu.name}-${index}`} style={{ borderBottom: index < gpus.length - 1 ? "1px solid var(--border-subtle)" : undefined, paddingBottom: index < gpus.length - 1 ? 16 : 0 }}>
            <div style={{ font: "var(--md-sys-typescale-title-small)", color: "var(--text-primary)", marginBottom: 10 }}>
              {gpu.name || `GPU ${index}`} {gpus.length > 1 && <span style={{ color: "var(--text-muted)" }}>#{index + 1}</span>}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(min(100%, 210px), 1fr))", gap: 12 }}>
              {metrics.map(([label, value]) => (
                <div key={label}>
                  <div style={{ color: "var(--text-muted)", font: "var(--md-sys-typescale-label-small)" }}>{label}</div>
                  <div className="font-mono" style={{ color: "var(--text-primary)", marginTop: 4 }}>{value}</div>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
