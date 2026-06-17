use std::collections::HashMap;
use std::time::Instant;

use crate::models::agent_metrics::{NetworkInterfaceInfo, NetworkTotal};
use crate::models::sse_payloads::{NetworkInterfaceRate, NetworkRate};

// ──────────────────────────────────────────────
// Network throughput calculation
// ──────────────────────────────────────────────

/// Compute bytes-per-second rate from cumulative counters (shared logic).
/// Returns (rx_rate, tx_rate). First call (no previous) returns (0, 0).
/// `saturating_sub` prevents underflow if counters reset (e.g. after reboot).
pub(super) fn delta_rate(
    current_rx: u64,
    current_tx: u64,
    prev: Option<&(u64, u64, Instant)>,
    now: Instant,
) -> (f64, f64) {
    if let Some(&(prev_rx, prev_tx, prev_time)) = prev {
        let elapsed = now.duration_since(prev_time).as_secs_f64();
        if elapsed > 0.0 {
            return (
                current_rx.saturating_sub(prev_rx) as f64 / elapsed,
                current_tx.saturating_sub(prev_tx) as f64 / elapsed,
            );
        }
    }
    (0.0, 0.0)
}

/// Convert cumulative aggregate byte counters into per-second throughput.
///
/// Prefers the agent-reported rate when the decoder observed those fields.
/// The explicit `rate_fields_present` marker matters because 0 B/s is a valid
/// new-agent reading, not proof that the fields were omitted by an older agent.
/// The `prev` baseline refreshes on every call regardless, so a later agent
/// downgrade keeps the fallback hot.
pub(super) fn compute_network_rate(
    network: &NetworkTotal,
    prev: &mut Option<(u64, u64, Instant)>,
) -> NetworkRate {
    let now = Instant::now();
    let (rx_fallback, tx_fallback) = delta_rate(
        network.total_rx_bytes,
        network.total_tx_bytes,
        prev.as_ref(),
        now,
    );
    *prev = Some((network.total_rx_bytes, network.total_tx_bytes, now));

    if network.rate_fields_present {
        NetworkRate {
            rx_bytes_per_sec: network.rx_bytes_per_sec,
            tx_bytes_per_sec: network.tx_bytes_per_sec,
            total_rx_bytes: network.total_rx_bytes,
            total_tx_bytes: network.total_tx_bytes,
        }
    } else {
        NetworkRate {
            rx_bytes_per_sec: rx_fallback,
            tx_bytes_per_sec: tx_fallback,
            total_rx_bytes: network.total_rx_bytes,
            total_tx_bytes: network.total_tx_bytes,
        }
    }
}

/// Convert per-interface cumulative byte counters into per-second rates.
/// Prunes stale entries for interfaces no longer reported by the agent.
pub(super) fn compute_interface_rates(
    interfaces: &[NetworkInterfaceInfo],
    prev_map: &mut HashMap<String, (u64, u64, Instant)>,
) -> Vec<NetworkInterfaceRate> {
    let now = Instant::now();
    let rates: Vec<NetworkInterfaceRate> = interfaces
        .iter()
        .map(|iface| {
            let (rx, tx) = delta_rate(
                iface.rx_bytes,
                iface.tx_bytes,
                prev_map.get(&iface.name),
                now,
            );
            prev_map.insert(iface.name.clone(), (iface.rx_bytes, iface.tx_bytes, now));
            NetworkInterfaceRate {
                name: iface.name.clone(),
                rx_bytes_per_sec: rx,
                tx_bytes_per_sec: tx,
            }
        })
        .collect();
    // Prune stale entries (removed interfaces, e.g. Docker veth teardown)
    prev_map.retain(|name, _| interfaces.iter().any(|i| i.name == *name));
    rates
}
