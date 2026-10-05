"use client";

import { Monitor } from "lucide-react";
import { GpuInfo } from "@/app/types/metrics";
import { useI18n } from "@/app/i18n/I18nContext";
import { meterTone } from "@/app/lib/status";
import { EmptyState } from "@/app/components/ui";

interface GpuCardProps {
  gpus: GpuInfo[];
}

/** Shown for a reading the driver did not report — distinct from a real 0. */
const MISSING = "—";

/** Usage % uses the shared 60/85 thresholds so a GPU at 72% reads the
 *  same as a CPU at 72% anywhere else in the app. */
function getUsageColor(pct: number): string {
  return `var(--${meterTone(pct)})`;
}

/** Temperature has its own scale — degrees Celsius, not a percentage —
 *  but maps onto the same three signal tones. */
function getTempColor(tempC: number): string {
  if (tempC < 60) return "var(--ok)";
  if (tempC <= 80) return "var(--warn)";
  return "var(--crit)";
}

function formatMemory(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${mb.toFixed(0)} MB`;
}

function formatWatts(watts: number | null | undefined): string {
  return watts != null ? `${watts.toFixed(1)} W` : MISSING;
}

export default function GpuCard({ gpus }: GpuCardProps) {
  const { t } = useI18n();

  if (!gpus || gpus.length === 0) {
    return (
      <EmptyState
        icon={<Monitor size={24} aria-hidden="true" />}
        title={t.gpu.noData}
      />
    );
  }

  return (
    <div className="gpu-list">
      {gpus.map((gpu, idx) => {
        // Every reading is nullable: NVML can fail one query (e.g. power on
        // some laptop GPUs) while the rest succeed.
        const usagePct = gpu.gpu_usage_percent != null ? Math.min(gpu.gpu_usage_percent, 100) : null;
        const used = gpu.memory_used_mb;
        const total = gpu.memory_total_mb != null && gpu.memory_total_mb > 0 ? gpu.memory_total_mb : null;
        const memoryPct = used != null && total != null ? (used / total) * 100 : null;
        const temp = gpu.temperature_c;

        return (
          <div key={`${gpu.name}-${idx}`} className="gpu-item">
            <div className="gpu-item__head">
              <span className="gpu-item__name">
                <Monitor size={13} aria-hidden="true" />
                {gpu.name || `GPU ${idx}`}
              </span>
              <span
                className="gpu-item__pct"
                style={usagePct != null ? { color: getUsageColor(usagePct) } : undefined}
              >
                {usagePct != null ? `${usagePct.toFixed(1)}%` : MISSING}
              </span>
            </div>

            <span className="inline-meter-bar">
              <span
                className="inline-meter-fill"
                style={{
                  width: `${usagePct ?? 0}%`,
                  background: usagePct != null ? getUsageColor(usagePct) : undefined,
                }}
              />
            </span>

            <div className="gpu-item__foot">
              <span className="mono">
                {t.gpu.memory}: {used != null ? formatMemory(used) : MISSING} / {total != null ? formatMemory(total) : MISSING} ({memoryPct != null ? `${memoryPct.toFixed(1)}%` : MISSING})
              </span>
              <span className="mono gpu-item__foot-right">
                <span style={temp != null ? { color: getTempColor(temp) } : undefined}>
                  {temp != null ? `${temp.toFixed(1)}°C` : MISSING}
                </span>
              </span>
            </div>
            <div className="gpu-item__foot">
              <span className="mono">
                {t.gpu.power}: {formatWatts(gpu.power_watts)} / {formatWatts(gpu.power_limit_watts)}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
