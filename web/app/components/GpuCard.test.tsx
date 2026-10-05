import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import GpuCard from "./GpuCard";

describe("GpuCard", () => {
  it("computes VRAM occupancy from used and total and separates devices", () => {
    const html = renderToStaticMarkup(
      <GpuCard gpus={[
        { name: "NVIDIA A", gpu_usage_percent: 30, memory_used_mb: 2048, memory_total_mb: 8192, temperature_c: 64, power_watts: 80, power_limit_watts: 200 },
        { name: "NVIDIA B", gpu_usage_percent: 50, memory_used_mb: 1024, memory_total_mb: 4096, temperature_c: 60, power_watts: 90, power_limit_watts: 150 },
      ]} />,
    );

    expect(html).toContain("2.0 GB / 8.0 GB (25.0%)");
    expect(html).toContain("NVIDIA B");
    expect(html).toContain("80.0 W / 200.0 W");
  });

  it("distinguishes missing GPU telemetry from a genuine zero reading", () => {
    const html = renderToStaticMarkup(
      <GpuCard gpus={[
        { name: "NVIDIA A", gpu_usage_percent: null, memory_used_mb: null, memory_total_mb: 4096, temperature_c: null, power_watts: null, power_limit_watts: null },
        { name: "NVIDIA B", gpu_usage_percent: 0, memory_used_mb: 0, memory_total_mb: 4096, temperature_c: 0, power_watts: 0, power_limit_watts: 120 },
      ]} />,
    );

    expect(html).toContain("0 MB / 4.0 GB (0.0%)");
    expect(html).toContain("0.0 W / 120.0 W");
    expect((html.match(/—/g) ?? []).length).toBeGreaterThanOrEqual(4);
  });
});
