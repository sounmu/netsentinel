use crate::models::agent_metrics::AgentMetrics;

/// Persist collected agent metrics to the database.
/// Batch-insert metrics for multiple hosts in a single query.
/// Generic over `SqliteExecutor` so a scrape cycle can group online and
/// offline batch inserts in one transaction.
pub async fn insert_metrics_batch<'e, E>(
    executor: E,
    batch: &[(&str, &AgentMetrics)],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    if batch.is_empty() {
        return Ok(());
    }

    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "INSERT INTO metrics (\
         host_key, display_name, is_online, \
         cpu_usage_percent, memory_usage_percent, \
         load_1min, load_5min, load_15min, \
         networks, docker_containers, ports, disks, \
         processes, temperatures, gpus, \
         cpu_cores, network_interfaces, docker_stats, \
         rx_bytes_per_sec, tx_bytes_per_sec, \
         total_rx_bytes, total_tx_bytes) ",
    );

    qb.push_values(batch, |mut b, (host_key, metrics)| {
        // SQLite stores INTEGER as i64; the agent-reported counters are u64.
        // Saturating-cast is the right behaviour: a host that has actually
        // moved >9 EB on a counter is theoretical-only, but i64 overflow on
        // the bind would error out the whole batch.
        let total_rx = i64::try_from(metrics.network.total_rx_bytes).unwrap_or(i64::MAX);
        let total_tx = i64::try_from(metrics.network.total_tx_bytes).unwrap_or(i64::MAX);
        b.push_bind(host_key.to_string())
            .push_bind(metrics.hostname.clone())
            .push_bind(metrics.is_online)
            .push_bind(metrics.system.cpu_usage_percent)
            .push_bind(metrics.system.memory_usage_percent)
            .push_bind(metrics.load_average.one_min as f32)
            .push_bind(metrics.load_average.five_min as f32)
            .push_bind(metrics.load_average.fifteen_min as f32)
            .push_bind(sqlx::types::Json(&metrics.network))
            .push_bind(sqlx::types::Json(&metrics.docker_containers))
            .push_bind(sqlx::types::Json(&metrics.ports))
            .push_bind(sqlx::types::Json(&metrics.system.disks))
            .push_bind(sqlx::types::Json(&metrics.system.processes))
            .push_bind(sqlx::types::Json(&metrics.system.temperatures))
            .push_bind(sqlx::types::Json(&metrics.system.gpus))
            .push_bind(sqlx::types::Json(&metrics.cpu_cores))
            .push_bind(sqlx::types::Json(&metrics.network_interfaces))
            .push_bind(sqlx::types::Json(&metrics.docker_stats))
            .push_bind(metrics.network.rx_bytes_per_sec)
            .push_bind(metrics.network.tx_bytes_per_sec)
            .push_bind(total_rx)
            .push_bind(total_tx);
    });

    qb.build().execute(executor).await?;
    Ok(())
}

/// Batch-insert offline metric records for multiple unreachable hosts.
pub async fn insert_offline_metrics_batch<'e, E>(
    executor: E,
    batch: &[(&str, &str)],
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    if batch.is_empty() {
        return Ok(());
    }

    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "INSERT INTO metrics (\
         host_key, display_name, is_online, \
         cpu_usage_percent, memory_usage_percent, \
         load_1min, load_5min, load_15min, \
         rx_bytes_per_sec, tx_bytes_per_sec) ",
    );
    qb.push_values(batch, |mut b, (host_key, display_name)| {
        b.push_bind(host_key.to_string())
            .push_bind(display_name.to_string())
            .push_bind(false)
            .push_bind(0f32)
            .push_bind(0f32)
            .push_bind(0f32)
            .push_bind(0f32)
            .push_bind(0f32)
            .push_bind(0f64)
            .push_bind(0f64);
    });
    qb.build().execute(executor).await?;
    Ok(())
}
