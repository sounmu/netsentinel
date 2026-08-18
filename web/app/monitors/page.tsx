"use client";

import { useState, useMemo } from "react";
import useSWR from "swr";
import { toast } from "sonner";
import { Globe, Wifi, Plus, Trash2 } from "lucide-react";
import {
  HttpMonitor, HttpMonitorSummary, PingMonitor, PingMonitorSummary,
  getHttpMonitorsUrl, getHttpSummariesUrl, getPingMonitorsUrl, getPingSummariesUrl,
  createHttpMonitor, deleteHttpMonitor, createPingMonitor, deletePingMonitor,
  fetcher,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import { uptimeTone } from "@/app/lib/status";
import { PageHeader } from "@/app/components/PageHeader";
import {
  Button, Panel, PanelHeader, Field, EmptyState, Segmented, StatusDot,
} from "@/app/components/ui";

type Tab = "http" | "ping";

/**
 * A monitor of either kind renders identically: dot · name/target ·
 * latency · uptime · delete. Both tabs previously duplicated this
 * markup, the uptime colour maths, and the empty state.
 */
interface MonitorRow {
  id: number;
  name: string;
  target: string;
  healthy: boolean;
  latency: string | null;
  uptimePct: number | null;
}

function MonitorList({
  rows,
  emptyLabel,
  onDelete,
  deleteLabel,
}: {
  rows: MonitorRow[];
  emptyLabel: string;
  onDelete: (id: number) => void;
  deleteLabel: string;
}) {
  if (rows.length === 0) {
    return (
      <Panel>
        <EmptyState icon={<Globe size={28} aria-hidden="true" />} title={emptyLabel} />
      </Panel>
    );
  }

  return (
    <div className="row-stack">
      {rows.map((row) => (
        <div key={row.id} className="record-row">
          <StatusDot tone={row.healthy ? "ok" : "crit"} firing={!row.healthy} />
          <div className="record-row__id">
            <span className="record-row__name">{row.name}</span>
            <span className="record-row__meta" title={row.target}>{row.target}</span>
          </div>
          {row.latency && <span className="record-row__num">{row.latency}</span>}
          {row.uptimePct !== null && (
            <span
              className="record-row__num"
              style={{ color: `var(--${uptimeTone(row.uptimePct)})` }}
            >
              {row.uptimePct.toFixed(1)}%
            </span>
          )}
          <div className="record-row__actions">
            <Button
              variant="danger"
              size="sm"
              icon
              aria-label={deleteLabel}
              title={deleteLabel}
              onClick={() => onDelete(row.id)}
            >
              <Trash2 size={13} aria-hidden="true" />
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}

export default function MonitorsPage() {
  const { t } = useI18n();
  const [activeTab, setActiveTab] = useState<Tab>("http");
  const [showForm, setShowForm] = useState(false);

  const { data: httpMonitors } = useSWR<HttpMonitor[]>(
    getHttpMonitorsUrl(), fetcher, { revalidateOnFocus: false },
  );
  const { data: pingMonitors } = useSWR<PingMonitor[]>(
    getPingMonitorsUrl(), fetcher, { revalidateOnFocus: false },
  );

  const total = (httpMonitors?.length ?? 0) + (pingMonitors?.length ?? 0);

  const tabs = [
    { value: "http" as const, label: t.monitors.httpMonitors, icon: <Globe size={13} aria-hidden="true" />, count: httpMonitors?.length ?? null },
    { value: "ping" as const, label: t.monitors.pingMonitors, icon: <Wifi size={13} aria-hidden="true" />, count: pingMonitors?.length ?? null },
  ];

  return (
    <div className="page-content fade-in">
      <PageHeader
        icon={<Globe size={16} aria-hidden="true" />}
        title={t.monitors.title}
        badge={total}
        description={t.monitors.description}
        right={
          <Button variant="primary" onClick={() => setShowForm((v) => !v)}>
            <Plus size={14} aria-hidden="true" /> {t.monitors.addMonitor}
          </Button>
        }
      />

      <Segmented
        options={tabs}
        value={activeTab}
        onChange={(next) => {
          setActiveTab(next);
          setShowForm(false);
        }}
        ariaLabel={t.monitors.title}
      />

      {activeTab === "http" ? (
        <HttpMonitorsTab showForm={showForm} onCloseForm={() => setShowForm(false)} />
      ) : (
        <PingMonitorsTab showForm={showForm} onCloseForm={() => setShowForm(false)} />
      )}
    </div>
  );
}

function HttpMonitorsTab({ showForm, onCloseForm }: { showForm: boolean; onCloseForm: () => void }) {
  const { t } = useI18n();
  const { data: monitors, mutate: mutateMonitors } = useSWR<HttpMonitor[]>(
    getHttpMonitorsUrl(), fetcher, { revalidateOnFocus: false },
  );
  const { data: summaries } = useSWR<HttpMonitorSummary[]>(
    getHttpSummariesUrl(), fetcher, { refreshInterval: 10000, revalidateOnFocus: false },
  );

  const [formName, setFormName] = useState("");
  const [formUrl, setFormUrl] = useState("");
  const [formMethod, setFormMethod] = useState("GET");
  const [formExpectedStatus, setFormExpectedStatus] = useState(200);
  const [formInterval, setFormInterval] = useState(60);
  const [formTimeout, setFormTimeout] = useState(10000);

  const summaryMap = useMemo(
    () => new Map(summaries?.map((s) => [s.monitor_id, s])),
    [summaries],
  );

  const rows: MonitorRow[] = (monitors ?? []).map((m) => {
    const summary = summaryMap.get(m.id);
    return {
      id: m.id,
      name: m.name,
      target: `${m.method} ${m.url}`,
      healthy: summary ? summary.latest_error === null : true,
      latency: summary?.latest_response_time_ms != null
        ? `${summary.latest_response_time_ms}ms`
        : null,
      uptimePct: summary ? summary.uptime_pct : null,
    };
  });

  const handleCreate = async () => {
    if (!formName.trim() || !formUrl.trim()) return;
    try {
      await createHttpMonitor({
        name: formName,
        url: formUrl,
        method: formMethod,
        expected_status: formExpectedStatus,
        interval_secs: formInterval,
        timeout_ms: formTimeout,
      });
      onCloseForm();
      setFormName("");
      setFormUrl("");
      setFormMethod("GET");
      setFormExpectedStatus(200);
      setFormInterval(60);
      setFormTimeout(10000);
      await mutateMonitors();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.monitors.addMonitor);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteHttpMonitor(id);
      await mutateMonitors();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.monitors.addMonitor);
    }
  };

  return (
    <>
      {showForm && (
        <Panel>
          <PanelHeader
            title={t.monitors.addMonitor}
            right={
              <Button variant="ghost" size="sm" onClick={onCloseForm}>
                {t.common.cancel}
              </Button>
            }
          />
          <div className="form-body">
            <div className="form-grid">
              <Field label={t.monitors.name} htmlFor="http-monitor-name">
                <input id="http-monitor-name" className="date-input" value={formName}
                  onChange={(e) => setFormName(e.target.value)} placeholder="My API" />
              </Field>
              <Field label={t.monitors.url} htmlFor="http-monitor-url">
                <input id="http-monitor-url" className="date-input" value={formUrl}
                  onChange={(e) => setFormUrl(e.target.value)} placeholder="https://example.com" />
              </Field>
              <Field label={t.monitors.method} htmlFor="http-monitor-method">
                <select id="http-monitor-method" className="date-input" value={formMethod}
                  onChange={(e) => setFormMethod(e.target.value)}>
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                  <option value="HEAD">HEAD</option>
                </select>
              </Field>
              <Field label={t.monitors.expectedStatus} htmlFor="http-monitor-expected-status">
                <input id="http-monitor-expected-status" className="date-input" type="number"
                  value={formExpectedStatus}
                  onChange={(e) => setFormExpectedStatus(parseInt(e.target.value) || 200)} />
              </Field>
              <Field label={t.monitors.interval} htmlFor="http-monitor-interval">
                <input id="http-monitor-interval" className="date-input" type="number"
                  value={formInterval}
                  onChange={(e) => setFormInterval(parseInt(e.target.value) || 60)} />
              </Field>
              <Field label={t.monitors.timeout} htmlFor="http-monitor-timeout">
                <input id="http-monitor-timeout" className="date-input" type="number"
                  value={formTimeout}
                  onChange={(e) => setFormTimeout(parseInt(e.target.value) || 10000)} />
              </Field>
            </div>
            <div className="form-actions">
              <Button variant="secondary" onClick={onCloseForm}>{t.common.cancel}</Button>
              <Button variant="primary" onClick={handleCreate}>{t.monitors.addMonitor}</Button>
            </div>
          </div>
        </Panel>
      )}

      <MonitorList
        rows={rows}
        emptyLabel={t.monitors.noMonitors}
        onDelete={handleDelete}
        deleteLabel={t.common.delete}
      />
    </>
  );
}

function PingMonitorsTab({ showForm, onCloseForm }: { showForm: boolean; onCloseForm: () => void }) {
  const { t } = useI18n();
  const { data: monitors, mutate: mutateMonitors } = useSWR<PingMonitor[]>(
    getPingMonitorsUrl(), fetcher, { revalidateOnFocus: false },
  );
  const { data: summaries } = useSWR<PingMonitorSummary[]>(
    getPingSummariesUrl(), fetcher, { refreshInterval: 10000, revalidateOnFocus: false },
  );

  const [formName, setFormName] = useState("");
  const [formHost, setFormHost] = useState("");

  const summaryMap = useMemo(
    () => new Map(summaries?.map((s) => [s.monitor_id, s])),
    [summaries],
  );

  const rows: MonitorRow[] = (monitors ?? []).map((m) => {
    const summary = summaryMap.get(m.id);
    return {
      id: m.id,
      name: m.name,
      target: m.host,
      healthy: summary ? summary.latest_success === true : true,
      latency: summary?.latest_rtt_ms != null ? `${summary.latest_rtt_ms.toFixed(1)}ms` : null,
      uptimePct: summary ? summary.uptime_pct : null,
    };
  });

  const handleCreate = async () => {
    if (!formName.trim() || !formHost.trim()) return;
    try {
      await createPingMonitor({ name: formName, host: formHost });
      onCloseForm();
      setFormName("");
      setFormHost("");
      await mutateMonitors();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.monitors.addMonitor);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deletePingMonitor(id);
      await mutateMonitors();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.monitors.addMonitor);
    }
  };

  return (
    <>
      {showForm && (
        <Panel>
          <PanelHeader
            title={t.monitors.addMonitor}
            right={
              <Button variant="ghost" size="sm" onClick={onCloseForm}>
                {t.common.cancel}
              </Button>
            }
          />
          <div className="form-body">
            <div className="form-grid">
              <Field label={t.monitors.name} htmlFor="ping-monitor-name">
                <input id="ping-monitor-name" className="date-input" value={formName}
                  onChange={(e) => setFormName(e.target.value)} placeholder="Gateway" />
              </Field>
              <Field label={t.monitors.host} htmlFor="ping-monitor-host">
                <input id="ping-monitor-host" className="date-input" value={formHost}
                  onChange={(e) => setFormHost(e.target.value)} placeholder="192.168.1.1" />
              </Field>
            </div>
            <div className="form-actions">
              <Button variant="secondary" onClick={onCloseForm}>{t.common.cancel}</Button>
              <Button variant="primary" onClick={handleCreate}>{t.monitors.addMonitor}</Button>
            </div>
          </div>
        </Panel>
      )}

      <MonitorList
        rows={rows}
        emptyLabel={t.monitors.noMonitors}
        onDelete={handleDelete}
        deleteLabel={t.common.delete}
      />
    </>
  );
}
