-- Static system information collected from agents on reconnection and every 24h.
-- Stored on the hosts table because this data is per-host metadata, not time-series.
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS os_info VARCHAR(255);
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS cpu_model VARCHAR(255);
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS memory_total_mb BIGINT;
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS boot_time BIGINT;
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS ip_address VARCHAR(45);
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS system_info_updated_at TIMESTAMPTZ;
