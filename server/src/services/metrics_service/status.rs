use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::models::agent_metrics::{DiskInfo, DockerContainer, PortStatus};

// ──────────────────────────────────────────────
// Status change detection
// ──────────────────────────────────────────────

/// Compute a hash of Docker container, port, and disk states.
/// Only fields that indicate a state change are included (name, state, port/open, disk usage rounded to 1%).
pub(super) fn compute_status_hash(
    containers: &[DockerContainer],
    ports: &[PortStatus],
    disks: &[DiskInfo],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    for c in containers {
        c.container_name.hash(&mut hasher);
        c.state.hash(&mut hasher);
        c.oom_killed.hash(&mut hasher);
        c.exit_code.hash(&mut hasher);
        c.restart_count.hash(&mut hasher);
        c.compose_project.hash(&mut hasher);
        c.compose_service.hash(&mut hasher);
        c.health_status.hash(&mut hasher);
    }
    for p in ports {
        p.port.hash(&mut hasher);
        p.is_open.hash(&mut hasher);
    }
    for d in disks {
        d.mount_point.hash(&mut hasher);
        // Round to 1% to avoid excessive SSE broadcasts from minor fluctuations
        (d.usage_percent as u32).hash(&mut hasher);
    }
    hasher.finish()
}
