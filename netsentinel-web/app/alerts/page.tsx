"use client";

import { useState, useCallback, useEffect } from "react";
import useSWR from "swr";
import { toast } from "sonner";
import { Bell, Save, Trash2, ChevronDown, ChevronUp, Plus, Send } from "lucide-react";
import {
  AlertConfigRow, UpsertAlertRequest, NotificationChannel, AlertHistoryRow,
  getAlertConfigsUrl, getHostAlertConfigsUrl, getHostsUrl, getNotificationChannelsUrl, getAlertHistoryUrl,
  fetcher, updateGlobalAlertConfigs, updateHostAlertConfigs, deleteHostAlertConfigs,
  createNotificationChannel, updateNotificationChannel, deleteNotificationChannel, testNotificationChannel,
} from "@/app/lib/api";
import { HostSummary } from "@/app/types/metrics";
import { useI18n } from "@/app/i18n/I18nContext";

interface AlertFormData {
  cpu_enabled: boolean;
  cpu_threshold: number;
  cpu_sustained_secs: number;
  cpu_cooldown_secs: number;
  memory_enabled: boolean;
  memory_threshold: number;
  memory_sustained_secs: number;
  memory_cooldown_secs: number;
  disk_enabled: boolean;
  disk_threshold: number;
  disk_sustained_secs: number;
  disk_cooldown_secs: number;
}

function configsToForm(configs: AlertConfigRow[]): AlertFormData {
  const cpu = configs.find((c) => c.metric_type === "cpu");
  const mem = configs.find((c) => c.metric_type === "memory");
  const disk = configs.find((c) => c.metric_type === "disk");
  return {
    cpu_enabled: cpu?.enabled ?? true,
    cpu_threshold: cpu?.threshold ?? 80,
    cpu_sustained_secs: cpu?.sustained_secs ?? 300,
    cpu_cooldown_secs: cpu?.cooldown_secs ?? 60,
    memory_enabled: mem?.enabled ?? true,
    memory_threshold: mem?.threshold ?? 90,
    memory_sustained_secs: mem?.sustained_secs ?? 300,
    memory_cooldown_secs: mem?.cooldown_secs ?? 60,
    disk_enabled: disk?.enabled ?? true,
    disk_threshold: disk?.threshold ?? 90,
    disk_sustained_secs: disk?.sustained_secs ?? 0,
    disk_cooldown_secs: disk?.cooldown_secs ?? 300,
  };
}

function formToRequests(form: AlertFormData): UpsertAlertRequest[] {
  return [
    { metric_type: "cpu", enabled: form.cpu_enabled, threshold: form.cpu_threshold, sustained_secs: form.cpu_sustained_secs, cooldown_secs: form.cpu_cooldown_secs },
    { metric_type: "memory", enabled: form.memory_enabled, threshold: form.memory_threshold, sustained_secs: form.memory_sustained_secs, cooldown_secs: form.memory_cooldown_secs },
    { metric_type: "disk", enabled: form.disk_enabled, threshold: form.disk_threshold, sustained_secs: form.disk_sustained_secs, cooldown_secs: form.disk_cooldown_secs },
  ];
}

export default function AlertsPage() {
  const { t } = useI18n();
  const { data: globalConfigs, mutate: mutateGlobal } = useSWR<AlertConfigRow[]>(
    getAlertConfigsUrl(), fetcher, { revalidateOnFocus: false }
  );
  const { data: hosts } = useSWR<HostSummary[]>(getHostsUrl(), fetcher, { revalidateOnFocus: false });

  const [globalForm, setGlobalForm] = useState<AlertFormData | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);

  useEffect(() => {
    if (globalConfigs) {
      setGlobalForm(configsToForm(globalConfigs));
    }
  }, [globalConfigs]);

  const handleGlobalSave = useCallback(async () => {
    if (!globalForm) return;
    setSaving(true);
    setSaveMsg(null);
    try {
      await updateGlobalAlertConfigs(formToRequests(globalForm));
      await mutateGlobal();
      setSaveMsg(t.alerts.globalSaved);
      setTimeout(() => setSaveMsg(null), 3000);
    } catch (e) {
      setSaveMsg(e instanceof Error ? e.message : t.alerts.saveFailed);
    } finally {
      setSaving(false);
    }
  }, [globalForm, mutateGlobal, t]);

  const saveFailed = saveMsg === t.alerts.saveFailed;

  return (
    <div className="page-content fade-in">
      <header className="alerts-header">
        <div className="alerts-header__title-row">
          <Bell size={20} color="var(--md-sys-color-primary)" aria-hidden="true" />
          <h1 className="alerts-header__title">{t.alerts.title}</h1>
        </div>
        <p className="alerts-header__description">{t.alerts.description}</p>
      </header>

      {/* Global defaults */}
      <section className="alerts-card alerts-card--padded">
        <div className="alerts-row alerts-row--between" style={{ marginBottom: "var(--md-sys-spacing-lg)" }}>
          <h2 className="alerts-section-title" style={{ marginBottom: 0 }}>
            {t.alerts.globalDefaults}
          </h2>
          <button
            type="button"
            onClick={handleGlobalSave}
            disabled={saving || !globalForm}
            className="alerts-btn alerts-btn--filled"
          >
            <Save size={14} aria-hidden="true" />
            {saving ? t.alerts.saving : t.alerts.save}
          </button>
        </div>

        {saveMsg && (
          <div
            role="status"
            aria-live="polite"
            className={`alerts-feedback ${saveFailed ? "alerts-feedback--error" : "alerts-feedback--success"}`}
          >
            {saveMsg}
          </div>
        )}

        {globalForm ? (
          <div className="alerts-metric-grid">
            <MetricAlertForm label={t.alerts.cpuAlert} prefix="cpu" form={globalForm} setForm={setGlobalForm} />
            <MetricAlertForm label={t.alerts.memoryAlert} prefix="memory" form={globalForm} setForm={setGlobalForm} />
            <MetricAlertForm label={t.alerts.diskAlert} prefix="disk" form={globalForm} setForm={setGlobalForm} />
          </div>
        ) : (
          <div className="skeleton" style={{ height: 200 }} />
        )}
      </section>

      {/* Per-host overrides */}
      <section className="alerts-section">
        <h2 className="alerts-section-title">{t.alerts.perHostOverrides}</h2>
        <p className="alerts-section-description">{t.alerts.perHostDescription}</p>

        {hosts?.map((host) => (
          <HostAlertOverride key={host.host_key} host={host} globalConfigs={globalConfigs ?? []} />
        ))}

        {(!hosts || hosts.length === 0) && (
          <div className="alerts-card alerts-card--empty">{t.alerts.noHosts}</div>
        )}
      </section>

      <AlertHistorySection />

      <NotificationChannelsSection />
    </div>
  );
}

/** Per-host alert override accordion */
function HostAlertOverride({ host, globalConfigs }: { host: HostSummary; globalConfigs: AlertConfigRow[] }) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const { data: hostConfigs, mutate } = useSWR<AlertConfigRow[]>(
    expanded ? getHostAlertConfigsUrl(host.host_key) : null,
    fetcher,
    { revalidateOnFocus: false }
  );

  const hasOverride = hostConfigs && hostConfigs.length > 0;
  const [form, setForm] = useState<AlertFormData | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    if (expanded && hostConfigs !== undefined) {
      const hasOvr = hostConfigs.length > 0;
      setForm(configsToForm(hasOvr ? hostConfigs : globalConfigs));
    }
  }, [expanded, hostConfigs, globalConfigs]);

  const handleSave = async () => {
    if (!form) return;
    setSaving(true);
    setMsg(null);
    try {
      await updateHostAlertConfigs(host.host_key, formToRequests(form));
      await mutate();
      setMsg(t.alerts.saved);
      setTimeout(() => setMsg(null), 3000);
    } catch (e) {
      setMsg(e instanceof Error ? e.message : t.alerts.saveFailed);
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteHostAlertConfigs(host.host_key);
      setForm(null);
      await mutate();
      setMsg(t.alerts.revertedToGlobal);
      setTimeout(() => setMsg(null), 3000);
    } catch {
      // silently ignore if no override existed
    }
  };

  const toggle = () => {
    setExpanded((v) => !v);
    if (expanded) { setForm(null); setMsg(null); }
  };

  const msgFailed = msg === t.alerts.saveFailed;

  return (
    <div className="alerts-card" style={{ overflow: "hidden" }}>
      <button
        type="button"
        onClick={toggle}
        className="alerts-host-toggle"
        aria-expanded={expanded}
      >
        <span
          className={`alerts-host-toggle__dot ${
            host.is_online
              ? "alerts-host-toggle__dot--online"
              : "alerts-host-toggle__dot--offline"
          }`}
          aria-hidden="true"
        />
        <div className="alerts-row__grow">
          <div className="alerts-host-toggle__name">{host.display_name}</div>
          <div className="alerts-host-toggle__key">{host.host_key}</div>
        </div>
        {hasOverride && (
          <span className="alerts-override-chip">{t.alerts.override}</span>
        )}
        {expanded
          ? <ChevronUp size={16} color="var(--md-sys-color-on-surface-variant)" aria-hidden="true" />
          : <ChevronDown size={16} color="var(--md-sys-color-on-surface-variant)" aria-hidden="true" />}
      </button>

      {expanded && form && (
        <div className="alerts-host-body">
          <div className="alerts-metric-grid alerts-host-body__grid">
            <MetricAlertForm label={t.alerts.cpu} prefix="cpu" form={form} setForm={setForm} />
            <MetricAlertForm label={t.alerts.memory} prefix="memory" form={form} setForm={setForm} />
            <MetricAlertForm label={t.alerts.disk} prefix="disk" form={form} setForm={setForm} />
          </div>

          {msg && (
            <div
              role="status"
              aria-live="polite"
              className={`alerts-feedback alerts-feedback--inline ${
                msgFailed ? "alerts-feedback--error" : "alerts-feedback--success"
              }`}
            >
              {msg}
            </div>
          )}

          <div className="alerts-row alerts-row--end alerts-row--tight" style={{ marginTop: "var(--md-sys-spacing-lg)" }}>
            {hasOverride && (
              <button
                type="button"
                onClick={handleDelete}
                className="alerts-btn alerts-btn--sm alerts-btn--danger"
              >
                <Trash2 size={12} aria-hidden="true" />
                {t.alerts.deleteOverride}
              </button>
            )}
            <button
              type="button"
              onClick={handleSave}
              disabled={saving}
              className="alerts-btn alerts-btn--sm alerts-btn--filled"
            >
              <Save size={12} aria-hidden="true" />
              {saving ? t.alerts.saving : t.alerts.save}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Reusable CPU/memory alert configuration form */
function MetricAlertForm({ label, prefix, form, setForm }: {
  label: string;
  prefix: "cpu" | "memory" | "disk";
  form: AlertFormData;
  setForm: React.Dispatch<React.SetStateAction<AlertFormData | null>>;
}) {
  const { t } = useI18n();
  const enabled = form[`${prefix}_enabled`];
  const threshold = form[`${prefix}_threshold`];
  const sustained = form[`${prefix}_sustained_secs`];
  const cooldown = form[`${prefix}_cooldown_secs`];

  const update = (field: string, value: number | boolean) => {
    setForm((prev) => prev ? { ...prev, [`${prefix}_${field}`]: value } : prev);
  };

  return (
    <div className={`alerts-metric ${enabled ? "" : "alerts-metric--disabled"}`}>
      <div className="alerts-metric__head">
        <span className="alerts-metric__label">{label}</span>
        <label className="alerts-metric__enable">
          <input
            type="checkbox"
            className="alerts-metric__enable-input"
            checked={enabled}
            onChange={(e) => update("enabled", e.target.checked)}
          />
          {t.alerts.enabled}
        </label>
      </div>
      <div className="alerts-metric__fields">
        <MiniField label={t.alerts.threshold} id={`alert-${prefix}-threshold`}>
          <input
            id={`alert-${prefix}-threshold`}
            className="alerts-field__input"
            type="number"
            step="0.1"
            value={threshold}
            onChange={(e) => update("threshold", parseFloat(e.target.value) || 0)}
          />
        </MiniField>
        <MiniField label={t.alerts.sustained} id={`alert-${prefix}-sustained`}>
          <input
            id={`alert-${prefix}-sustained`}
            className="alerts-field__input"
            type="number"
            value={sustained}
            onChange={(e) => update("sustained_secs", parseInt(e.target.value) || 0)}
          />
        </MiniField>
        <MiniField label={t.alerts.cooldown} id={`alert-${prefix}-cooldown`}>
          <input
            id={`alert-${prefix}-cooldown`}
            className="alerts-field__input"
            type="number"
            value={cooldown}
            onChange={(e) => update("cooldown_secs", parseInt(e.target.value) || 0)}
          />
        </MiniField>
      </div>
    </div>
  );
}

function MiniField({ label, id, children }: { label: string; id?: string; children: React.ReactNode }) {
  return (
    <div>
      <label htmlFor={id} className="alerts-field__label">{label}</label>
      {children}
    </div>
  );
}

/** Alert history feed */
function AlertHistorySection() {
  const { t, locale } = useI18n();
  const { data: alerts } = useSWR<AlertHistoryRow[]>(
    getAlertHistoryUrl(undefined, 30), fetcher,
    { refreshInterval: 30000, revalidateOnFocus: false }
  );

  const alertTypeEmoji: Record<string, string> = {
    cpu_overload: "🔥", cpu_recovery: "✅",
    memory_overload: "🔥", memory_recovery: "✅",
    disk_overload: "💾", disk_recovery: "✅",
    load_overload: "⚡", load_recovery: "✅",
    port_down: "🚫", port_recovery: "✅",
    host_down: "🔴", host_recovery: "✅",
  };

  return (
    <section className="alerts-section">
      <h2 className="alerts-section-title">{t.alertHistory.title}</h2>

      <div className="alerts-card alerts-history-list">
        {(!alerts || alerts.length === 0) && (
          <div className="alerts-card--empty" style={{ padding: "var(--md-sys-spacing-xl)" }}>
            {t.alertHistory.noAlerts}
          </div>
        )}

        {alerts?.map((alert) => (
          <div key={alert.id} className="alerts-history-row">
            <span className="alerts-history-row__icon" aria-hidden="true">
              {alertTypeEmoji[alert.alert_type] ?? "🔔"}
            </span>
            <div className="alerts-row__grow">
              <div className="alerts-history-row__message">
                {alert.message.replace(/\*\*/g, "").replace(/`/g, "")}
              </div>
              <div className="alerts-history-row__meta">
                <span className="alerts-history-row__host-key">{alert.host_key}</span>
                {" · "}
                {new Date(alert.created_at).toLocaleString(locale === "ko" ? "ko-KR" : "en-US")}
              </div>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

/** Notification channels management section */
function NotificationChannelsSection() {
  const { t } = useI18n();
  const { data: channels, mutate } = useSWR<NotificationChannel[]>(
    getNotificationChannelsUrl(), fetcher, { revalidateOnFocus: false }
  );
  const [showForm, setShowForm] = useState(false);
  const [formType, setFormType] = useState<"discord" | "slack" | "email">("discord");
  const [formName, setFormName] = useState("");
  const [formConfig, setFormConfig] = useState<Record<string, string>>({});
  const [testMsg, setTestMsg] = useState<Record<number, string>>({});

  const handleCreate = async () => {
    if (!formName.trim()) return;
    try {
      await createNotificationChannel({
        name: formName,
        channel_type: formType,
        config: formConfig,
      });
      setShowForm(false);
      setFormName("");
      setFormConfig({});
      await mutate();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.notifications.testFailed);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteNotificationChannel(id);
      await mutate();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.notifications.testFailed);
    }
  };

  const handleToggle = async (ch: NotificationChannel) => {
    await updateNotificationChannel(ch.id, { enabled: !ch.enabled });
    await mutate();
  };

  const handleTest = async (id: number) => {
    try {
      await testNotificationChannel(id);
      setTestMsg((prev) => ({ ...prev, [id]: t.notifications.testSuccess }));
    } catch {
      setTestMsg((prev) => ({ ...prev, [id]: t.notifications.testFailed }));
    }
    setTimeout(() => setTestMsg((prev) => { const n = { ...prev }; delete n[id]; return n; }), 3000);
  };

  const configFields = formType === "email"
    ? ["smtp_host", "smtp_port", "smtp_user", "smtp_pass", "from", "to"]
    : ["webhook_url"];

  const configLabels: Record<string, string> = {
    webhook_url: t.notifications.webhookUrl,
    smtp_host: t.notifications.smtpHost,
    smtp_port: t.notifications.smtpPort,
    smtp_user: t.notifications.smtpUser,
    smtp_pass: t.notifications.smtpPass,
    from: t.notifications.emailFrom,
    to: t.notifications.emailTo,
  };

  return (
    <section className="alerts-section">
      <div className="alerts-row alerts-row--between" style={{ marginBottom: "var(--md-sys-spacing-sm)" }}>
        <h2 className="alerts-section-title" style={{ marginBottom: 0 }}>
          {t.notifications.title}
        </h2>
        <button
          type="button"
          onClick={() => setShowForm((v) => !v)}
          className="alerts-btn alerts-btn--sm alerts-btn--filled"
        >
          <Plus size={14} aria-hidden="true" />
          {t.notifications.addChannel}
        </button>
      </div>

      <p className="alerts-section-description">{t.notifications.description}</p>

      {showForm && (
        <div className="alerts-card alerts-card--padded" style={{ marginBottom: "var(--md-sys-spacing-md)" }}>
          <div className="alerts-form-grid-2">
            <MiniField label={t.notifications.channelName} id="notif-channel-name">
              <input
                id="notif-channel-name"
                className="alerts-field__input"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder="My Slack"
              />
            </MiniField>
            <MiniField label={t.notifications.channelType} id="notif-channel-type">
              <select
                id="notif-channel-type"
                className="alerts-field__input"
                value={formType}
                onChange={(e) => { setFormType(e.target.value as "discord" | "slack" | "email"); setFormConfig({}); }}
              >
                <option value="discord">Discord</option>
                <option value="slack">Slack</option>
                <option value="email">Email</option>
              </select>
            </MiniField>
          </div>
          <div className="alerts-form-grid-auto">
            {configFields.map((field) => (
              <MiniField key={field} label={configLabels[field] ?? field} id={`notif-${field}`}>
                <input
                  id={`notif-${field}`}
                  className="alerts-field__input"
                  type={field === "smtp_pass" ? "password" : "text"}
                  value={formConfig[field] ?? ""}
                  onChange={(e) => setFormConfig((prev) => ({ ...prev, [field]: e.target.value }))}
                />
              </MiniField>
            ))}
          </div>
          <div className="alerts-row alerts-row--end alerts-row--tight">
            <button
              type="button"
              onClick={() => setShowForm(false)}
              className="alerts-btn alerts-btn--sm alerts-btn--tonal"
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              onClick={handleCreate}
              className="alerts-btn alerts-btn--sm alerts-btn--filled"
            >
              <Save size={12} aria-hidden="true" />
              {t.alerts.save}
            </button>
          </div>
        </div>
      )}

      {channels?.map((ch) => (
        <div key={ch.id} className="alerts-card alerts-channel-card">
          <button
            type="button"
            role="switch"
            aria-checked={ch.enabled}
            aria-label={ch.name}
            onClick={() => handleToggle(ch)}
            className="switch"
          />
          <div className="alerts-row__grow">
            <div className="alerts-channel__name">{ch.name}</div>
            <div className="alerts-channel__type">{ch.channel_type}</div>
          </div>
          {testMsg[ch.id] && (
            <span
              className={`alerts-channel__test-msg ${
                testMsg[ch.id] === t.notifications.testSuccess
                  ? "alerts-channel__test-msg--success"
                  : "alerts-channel__test-msg--error"
              }`}
            >
              {testMsg[ch.id]}
            </span>
          )}
          <button
            type="button"
            onClick={() => handleTest(ch.id)}
            className="alerts-btn alerts-btn--sm alerts-btn--tonal"
            aria-label={`${t.notifications.testSend}: ${ch.name}`}
          >
            <Send size={10} aria-hidden="true" />
            {t.notifications.testSend}
          </button>
          <button
            type="button"
            onClick={() => handleDelete(ch.id)}
            className="alerts-icon-btn alerts-icon-btn--danger"
            aria-label={`Delete ${ch.name}`}
          >
            <Trash2 size={14} aria-hidden="true" />
          </button>
        </div>
      ))}

      {(!channels || channels.length === 0) && !showForm && (
        <div className="alerts-card alerts-card--empty">{t.notifications.noChannels}</div>
      )}
    </section>
  );
}
