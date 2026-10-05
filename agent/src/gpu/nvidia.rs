use crate::models::GpuInfo;

/// Collect NVIDIA GPU metrics via NVML. Returns empty vec if no NVIDIA GPU or driver is available.
pub fn collect() -> Vec<GpuInfo> {
    let nvml = match nvml_wrapper::Nvml::init() {
        Ok(n) => n,
        Err(_) => return vec![],
    };
    let count = match nvml.device_count() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    (0..count)
        .filter_map(|i| {
            let device = nvml.device_by_index(i).ok()?;
            // `unwrap_or_default()` previously yielded an empty string on
            // NVML driver bugs, which propagated through the dashboard as a
            // blank row. `GPU {i}` keeps the UI labelled so operators can
            // still spot which device is misbehaving even without a real
            // product name.
            let name = device.name().unwrap_or_else(|_| format!("GPU {i}"));
            let utilization = device.utilization_rates().ok();
            let memory = device.memory_info().ok();
            let temp = device
                .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                .ok();
            Some(GpuInfo {
                name,
                gpu_usage_percent: utilization.map(|rates| rates.gpu),
                memory_used_mb: memory.as_ref().map(|info| info.used / 1024 / 1024),
                memory_total_mb: memory.map(|info| info.total / 1024 / 1024),
                temperature_c: temp,
                power_watts: device.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                power_limit_watts: device
                    .power_management_limit()
                    .ok()
                    .map(|mw| mw as f32 / 1000.0),
            })
        })
        .collect()
}
