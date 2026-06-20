import { performance } from "node:perf_hooks";

const DEFAULTS = {
  hosts: 100,
  containersPerHost: 5,
  chartDays: 30,
  chartStepMinutes: 15,
  iterations: 250,
  warmup: 30,
};

function readArgs() {
  const args = { ...DEFAULTS };
  for (const raw of process.argv.slice(2)) {
    const match = raw.match(/^--([^=]+)=(.+)$/);
    if (!match) continue;
    const key = match[1];
    const value = Number(match[2]);
    if (key in args && Number.isFinite(value) && value > 0) {
      args[key] = value;
    }
  }
  return args;
}

function mulberry32(seed) {
  return function next() {
    let t = seed += 0x6d2b79f5;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function pickMetric(rng, min, max, decimals = 1) {
  const n = min + rng() * (max - min);
  const p = 10 ** decimals;
  return Math.round(n * p) / p;
}

function isoFromMs(ms) {
  return new Date(ms).toISOString();
}

function generateFixture({ hosts, containersPerHost, chartDays, chartStepMinutes }) {
  const rng = mulberry32(0x515151);
  const now = Date.now();
  const metricsMap = {};
  const statusMap = {};

  for (let h = 0; h < hosts; h++) {
    const hostKey = `host-${String(h + 1).padStart(3, "0")}.example:9101`;
    const displayName = `host-${String(h + 1).padStart(3, "0")}`;
    const isOnline = h % 17 !== 0;
    const dockerContainers = [];
    const dockerStats = [];
    for (let c = 0; c < containersPerHost; c++) {
      const name = `svc-${String(c + 1).padStart(2, "0")}`;
      dockerContainers.push({
        container_name: name,
        image: `ghcr.io/example/${name}:latest`,
        state: c % 19 === 0 && h % 11 === 0 ? "exited" : "running",
        status: "Up 2 hours",
        health_status: c % 23 === 0 ? "unhealthy" : "healthy",
        oom_killed: false,
        exit_code: null,
        restart_count: c % 13 === 0 ? 2 : 0,
      });
      dockerStats.push({
        container_name: name,
        cpu_percent: pickMetric(rng, 0, 95, 2),
        memory_usage_mb: Math.round(pickMetric(rng, 48, 2048, 0)),
        memory_limit_mb: 4096,
        net_rx_bytes: Math.round(pickMetric(rng, 1e6, 3e9, 0)),
        net_tx_bytes: Math.round(pickMetric(rng, 1e6, 3e9, 0)),
        block_read_bytes: Math.round(pickMetric(rng, 1e6, 8e9, 0)),
        block_write_bytes: Math.round(pickMetric(rng, 1e6, 8e9, 0)),
      });
    }

    statusMap[hostKey] = {
      host_key: hostKey,
      display_name: displayName,
      is_online: isOnline,
      last_seen: isoFromMs(now - h * 1000),
      scrape_interval_secs: 10,
      disks: [
        { name: "root", mount_point: "/", usage_percent: pickMetric(rng, 15, 92, 1) },
        { name: "data", mount_point: "/data", usage_percent: pickMetric(rng, 15, 92, 1) },
      ],
      docker_containers: dockerContainers,
      docker_stats: dockerStats,
    };

    metricsMap[hostKey] = {
      host_key: hostKey,
      display_name: displayName,
      timestamp: isoFromMs(now - h * 1000),
      is_online: isOnline,
      cpu_usage_percent: pickMetric(rng, 0, 98, 1),
      memory_usage_percent: pickMetric(rng, 10, 96, 1),
      load_1min: pickMetric(rng, 0, 12, 2),
      network_rate: {
        rx_bytes_per_sec: Math.round(pickMetric(rng, 0, 100_000_000, 0)),
        tx_bytes_per_sec: Math.round(pickMetric(rng, 0, 100_000_000, 0)),
      },
    };
  }

  const chartRows = [];
  const points = Math.ceil((chartDays * 24 * 60) / chartStepMinutes);
  const base = now - chartDays * 24 * 60 * 60 * 1000;
  for (let i = 0; i < points; i++) {
    const ts = base + i * chartStepMinutes * 60 * 1000;
    const dockerStats = [];
    for (let c = 0; c < containersPerHost; c++) {
      dockerStats.push({
        container_name: `svc-${String(c + 1).padStart(2, "0")}`,
        cpu_percent: pickMetric(rng, 0, 95, 2),
        memory_usage_mb: Math.round(pickMetric(rng, 48, 2048, 0)),
      });
    }
    chartRows.push({
      timestamp: isoFromMs(ts),
      cpu_usage_percent: pickMetric(rng, 0, 98, 1),
      memory_usage_percent: pickMetric(rng, 10, 96, 1),
      networks: {
        rx_bytes_per_sec: Math.round(pickMetric(rng, 0, 100_000_000, 0)),
        tx_bytes_per_sec: Math.round(pickMetric(rng, 0, 100_000_000, 0)),
        total_rx_bytes: i * 1000,
        total_tx_bytes: i * 500,
      },
      disks: [
        { name: "root", mount_point: "/", usage_percent: pickMetric(rng, 15, 92, 1), read_bytes_per_sec: 1000, write_bytes_per_sec: 500 },
        { name: "data", mount_point: "/data", usage_percent: pickMetric(rng, 15, 92, 1), read_bytes_per_sec: 2000, write_bytes_per_sec: 800 },
      ],
      docker_stats: dockerStats,
      temperatures: [{ label: "CPU Package", temperature_c: pickMetric(rng, 35, 88, 1) }],
    });
  }

  return { metricsMap, statusMap, chartRows };
}

function getHostStatus(lastSeen, isOnline, scrapeIntervalSecs) {
  if (!isOnline || !lastSeen) return "offline";
  const ageSecs = (Date.now() - new Date(lastSeen).getTime()) / 1000;
  if (ageSecs > scrapeIntervalSecs * 3) return "pending";
  return "online";
}

function deriveOverviewRows(metricsMap, statusMap) {
  const list = Object.values(statusMap).map((status) => {
    const metrics = metricsMap[status.host_key];
    const lastSeen = metrics?.timestamp ?? status.last_seen ?? null;
    const isOnline = metrics?.is_online ?? status.is_online ?? false;
    const hostStatus = getHostStatus(lastSeen, isOnline, status.scrape_interval_secs);
    const disks = status.disks ?? [];
    let diskPct = 0;
    if (disks.length > 0) {
      const root = disks.find((d) => d.mount_point === "/");
      diskPct = root ? root.usage_percent : Math.max(...disks.map((d) => d.usage_percent));
    }
    return {
      host_key: status.host_key,
      display_name: metrics?.display_name ?? status.display_name,
      status: hostStatus,
      cpu: metrics?.cpu_usage_percent ?? 0,
      ram: metrics?.memory_usage_percent ?? 0,
      disk: diskPct,
      load: metrics?.load_1min ?? 0,
      networkRx: metrics?.network_rate?.rx_bytes_per_sec ?? 0,
      networkTx: metrics?.network_rate?.tx_bytes_per_sec ?? 0,
    };
  });
  list.sort((a, b) => {
    const order = { online: 0, pending: 1, offline: 2 };
    const diff = order[a.status] - order[b.status];
    if (diff !== 0) return diff;
    return a.display_name.localeCompare(b.display_name);
  });
  return list;
}

function getContainerHealth(container) {
  if (container.state !== "running" || container.health_status === "unhealthy" || container.oom_killed) {
    return "attention";
  }
  return container.state === "running" ? "running" : "stopped";
}

function deriveContainerRows(metricsMap, statusMap) {
  const collected = [];
  let runningCount = 0;
  let attentionCount = 0;

  for (const status of Object.values(statusMap)) {
    const metrics = metricsMap[status.host_key];
    const lastSeen = metrics?.timestamp ?? status.last_seen ?? null;
    const isOnline = metrics?.is_online ?? status.is_online ?? false;
    const hostStatus = getHostStatus(lastSeen, isOnline, status.scrape_interval_secs);
    const statsByName = new Map();
    for (const stat of status.docker_stats ?? []) {
      statsByName.set(stat.container_name, stat);
    }

    for (const container of status.docker_containers ?? []) {
      const stat = statsByName.get(container.container_name);
      const memoryPercent = stat && stat.memory_limit_mb > 0
        ? (stat.memory_usage_mb / stat.memory_limit_mb) * 100
        : null;
      const health = getContainerHealth(container);
      if (health === "running") runningCount += 1;
      if (health === "attention") attentionCount += 1;
      collected.push({
        key: `${status.host_key}::${container.container_name}`,
        hostKey: status.host_key,
        hostDisplayName: metrics?.display_name ?? status.display_name,
        hostStatus,
        container,
        stat,
        memoryPercent,
        health,
      });
    }
  }

  collected.sort((a, b) => {
    const healthOrder = { attention: 0, running: 1, stopped: 2 };
    const statusOrder = { online: 0, pending: 1, offline: 2 };
    const healthDiff = healthOrder[a.health] - healthOrder[b.health];
    if (healthDiff !== 0) return healthDiff;
    const statusDiff = statusOrder[a.hostStatus] - statusOrder[b.hostStatus];
    if (statusDiff !== 0) return statusDiff;
    return a.container.container_name.localeCompare(b.container.container_name);
  });

  return {
    rows: collected,
    total: collected.length,
    running: runningCount,
    attention: attentionCount,
    hostCount: new Set(collected.map((row) => row.hostKey)).size,
  };
}

function pickCpuTemp(temps) {
  if (!temps || temps.length === 0) return null;
  for (const p of ["package", "tctl", "tdie", "cpu"]) {
    const found = temps.find((t) => t.label.toLowerCase().includes(p) && t.temperature_c > 0);
    if (found) return found;
  }
  return temps.reduce((a, b) => (b.temperature_c > a.temperature_c ? b : a), temps[0]);
}

function projectChartData(rows) {
  const sorted = [...rows].sort(
    (a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime(),
  );
  const cpu = [];
  const ram = [];
  const net = [];
  const diskUsageNames = new Set();
  const diskUsageData = [];
  const diskIo = [];
  const tempData = [];
  const dockerCpuNames = new Set();
  const dockerCpuData = [];
  const dockerMemNames = new Set();
  const dockerMemData = [];

  for (let i = 0; i < sorted.length; i++) {
    const r = sorted[i];
    const tsMs = new Date(r.timestamp).getTime();
    cpu.push({ ts: tsMs, "CPU (%)": +r.cpu_usage_percent.toFixed(1) });
    ram.push({ ts: tsMs, "RAM (%)": +r.memory_usage_percent.toFixed(1) });

    const currNet = r.networks;
    if (
      currNet &&
      typeof currNet.rx_bytes_per_sec === "number" &&
      typeof currNet.tx_bytes_per_sec === "number"
    ) {
      net.push({
        ts: tsMs,
        RX: +currNet.rx_bytes_per_sec.toFixed(0),
        TX: +currNet.tx_bytes_per_sec.toFixed(0),
      });
    }

    const disks = r.disks;
    if (disks && disks.length > 0) {
      const uPoint = { ts: tsMs };
      let totalRead = 0;
      let totalWrite = 0;
      for (const d of disks) {
        const label = d.mount_point || d.name;
        diskUsageNames.add(label);
        uPoint[label] = +d.usage_percent.toFixed(1);
        totalRead += d.read_bytes_per_sec ?? 0;
        totalWrite += d.write_bytes_per_sec ?? 0;
      }
      diskUsageData.push(uPoint);
      diskIo.push({ ts: tsMs, Read: +totalRead.toFixed(0), Write: +totalWrite.toFixed(0) });
    }

    const dStats = r.docker_stats;
    if (dStats && dStats.length > 0) {
      const cpuPt = { ts: tsMs };
      const memPt = { ts: tsMs };
      for (const ds of dStats) {
        dockerCpuNames.add(ds.container_name);
        dockerMemNames.add(ds.container_name);
        cpuPt[ds.container_name] = +ds.cpu_percent.toFixed(2);
        memPt[ds.container_name] = ds.memory_usage_mb;
      }
      dockerCpuData.push(cpuPt);
      dockerMemData.push(memPt);
    }

    const main = pickCpuTemp(r.temperatures);
    if (main && main.temperature_c > 0) {
      tempData.push({ ts: tsMs, "CPU Temp": +main.temperature_c.toFixed(1) });
    }
  }

  return {
    cpu,
    ram,
    net,
    diskUsageData,
    diskUsageKeys: [...diskUsageNames],
    diskIo,
    dockerCpuData,
    dockerCpuKeys: [...dockerCpuNames],
    dockerMemData,
    dockerMemKeys: [...dockerMemNames],
    tempData,
  };
}

function summarize(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const sum = sorted.reduce((a, b) => a + b, 0);
  const at = (p) => sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];
  return {
    min: sorted[0],
    mean: sum / sorted.length,
    p50: at(0.5),
    p95: at(0.95),
    max: sorted[sorted.length - 1],
  };
}

function bench(name, fn, { iterations, warmup }) {
  for (let i = 0; i < warmup; i++) fn();
  const samples = [];
  let last;
  for (let i = 0; i < iterations; i++) {
    const start = performance.now();
    last = fn();
    samples.push(performance.now() - start);
  }
  return { name, stats: summarize(samples), last };
}

function formatMs(n) {
  return `${n.toFixed(3)} ms`;
}

function printResult(result) {
  const s = result.stats;
  console.log([
    result.name.padEnd(22),
    `min ${formatMs(s.min)}`,
    `mean ${formatMs(s.mean)}`,
    `p50 ${formatMs(s.p50)}`,
    `p95 ${formatMs(s.p95)}`,
    `max ${formatMs(s.max)}`,
  ].join(" | "));
}

const args = readArgs();
const fixture = generateFixture(args);

console.log("NetSentinel web synthetic benchmark");
console.log(JSON.stringify({
  hosts: args.hosts,
  containersPerHost: args.containersPerHost,
  totalContainers: args.hosts * args.containersPerHost,
  chartDays: args.chartDays,
  chartStepMinutes: args.chartStepMinutes,
  chartRows: fixture.chartRows.length,
  iterations: args.iterations,
  warmup: args.warmup,
}));

const results = [
  bench("overview rows", () => deriveOverviewRows(fixture.metricsMap, fixture.statusMap), args),
  bench("container rows", () => deriveContainerRows(fixture.metricsMap, fixture.statusMap), args),
  bench("chart projection", () => projectChartData(fixture.chartRows), args),
];

for (const result of results) {
  printResult(result);
}

console.log(JSON.stringify({
  overviewRows: results[0].last.length,
  containerRows: results[1].last.total,
  chartSeries: {
    cpu: results[2].last.cpu.length,
    dockerCpuKeys: results[2].last.dockerCpuKeys.length,
    diskUsageKeys: results[2].last.diskUsageKeys.length,
  },
  heapUsedMb: Math.round(process.memoryUsage().heapUsed / 1024 / 1024),
}));
