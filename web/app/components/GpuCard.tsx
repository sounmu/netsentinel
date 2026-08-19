"use client";

import { Monitor } from "lucide-react";
import { GpuInfo } from "@/app/types/metrics";
import { useI18n } from "@/app/i18n/I18nContext";
import { meterTone } from "@/app/lib/status";
import { EmptyState } from "@/app/components/ui";

interface GpuCardProps {
  gpus: GpuInfo[];
}

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
        const usagePct = Math.min(gpu.gpu_usage_percent, 100);
        const usageColor = getUsageColor(usagePct);
        const tempColor = getTempColor(gpu.temperature_c);

        return (
          <div key={`${gpu.name}-${idx}`} className="gpu-item">
            <div className="gpu-item__head">
              <span className="gpu-item__name">
                <Monitor size={13} aria-hidden="true" />
                {gpu.name}
              </span>
              <span className="gpu-item__pct" style={{ color: usageColor }}>
                {usagePct.toFixed(1)}%
              </span>
            </div>

            <span className="inline-meter-bar">
              <span
                className="inline-meter-fill"
                style={{ width: `${usagePct}%`, background: usageColor }}
              />
            </span>

            <div className="gpu-item__foot">
              <span className="mono">
                {gpu.memory_total_mb > 0
                  ? `${t.gpu.memory}: ${formatMemory(gpu.memory_used_mb)} / ${formatMemory(gpu.memory_total_mb)}`
                  : gpu.power_watts != null
                    ? `${t.gpu.power}: ${gpu.power_watts.toFixed(1)} W`
                    : "—"}
              </span>
              <span className="mono gpu-item__foot-right">
                {gpu.frequency_mhz != null && <span>{gpu.frequency_mhz} MHz</span>}
                <span style={{ color: tempColor }}>
                  {gpu.temperature_c.toFixed(1)}°C
                </span>
              </span>
            </div>
            {/* Show power below memory for NVIDIA GPUs that have both */}
            {gpu.memory_total_mb > 0 && gpu.power_watts != null && (
              <div className="gpu-item__foot">
                <span className="mono">
                  {t.gpu.power}: {gpu.power_watts.toFixed(1)} W
                </span>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
