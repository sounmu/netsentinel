"use client";

import { useState, useCallback } from "react";
import useSWR from "swr";
import {
  Settings, Plus, Pencil, Trash2, Server, Save, X, AlertTriangle, Copy, RefreshCw,
  Terminal,
} from "lucide-react";
import {
  AgentEnrollmentToken, HostConfig, createAgentEnrollment, getApiBase, getHostsUrl, fetcher,
  getHostConfig, createHost, updateHost, deleteHost,
} from "@/app/lib/api";
import { HostSummary } from "@/app/types/metrics";
import { useI18n } from "@/app/i18n/I18nContext";
import { useRemoveHost } from "@/app/lib/sse-context";
import { toast } from "sonner";
import { PageHeader } from "@/app/components/PageHeader";
import {
  Button, Panel, PanelHeader, Field, EmptyState, SkeletonRows, StatusDot, Badge,
} from "@/app/components/ui";

/** Host form data */
interface HostFormData {
  host_key: string;
  display_name: string;
  scrape_interval_secs: number;
  load_threshold: number;
  ports: string; // comma-separated string
  containers: string;
}

type InstallNetwork = "lan" | "tailscale";

const emptyForm: HostFormData = {
  host_key: "",
  display_name: "",
  scrape_interval_secs: 10,
  load_threshold: 4.0,
  ports: "80, 443",
  containers: "",
};

function hostToForm(h: HostConfig): HostFormData {
  return {
    host_key: h.host_key,
    display_name: h.display_name,
    scrape_interval_secs: h.scrape_interval_secs,
    load_threshold: h.load_threshold,
    ports: h.ports.join(", "),
    containers: h.containers.join(", "),
  };
}

function parsePorts(s: string): number[] {
  return s.split(",").map((p) => parseInt(p.trim(), 10)).filter((n) => !isNaN(n));
}

function parseContainers(s: string): string[] {
  return s.split(",").map((c) => c.trim()).filter(Boolean);
}

function defaultServerUrl() {
  const apiBase = getApiBase();
  if (apiBase.startsWith("http://") || apiBase.startsWith("https://")) {
    return apiBase.replace(/\/$/, "");
  }
  if (typeof window !== "undefined") {
    return window.location.origin;
  }
  return "";
}

function shellArg(value: string) {
  return `"${value.replace(/(["\\$`])/g, "\\$1")}"`;
}

export default function AgentsPage() {
  const { t } = useI18n();
  const removeHost = useRemoveHost();
  const { data: hosts, isLoading, error, mutate } = useSWR<HostSummary[]>(
    getHostsUrl(), fetcher, { revalidateOnFocus: false }
  );

  const [editingKey, setEditingKey] = useState<string | null>(null); // null=list, "new"=add, host_key=edit
  const [form, setForm] = useState<HostFormData>(emptyForm);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [enrollment, setEnrollment] = useState<AgentEnrollmentToken | null>(null);
  const [enrollmentLoading, setEnrollmentLoading] = useState(false);
  const [serverUrl, setServerUrl] = useState("");
  const [installNetwork, setInstallNetwork] = useState<InstallNetwork>("lan");
  const [agentPort, setAgentPort] = useState(9101);
  const [copied, setCopied] = useState(false);

  const issueEnrollment = useCallback(async () => {
    setEnrollmentLoading(true);
    setFormError(null);
    try {
      const token = await createAgentEnrollment({ label: "Agent install", ttl_secs: 900 });
      setEnrollment(token);
      setCopied(false);
    } catch (e) {
      setEnrollment(null);
      setFormError(e instanceof Error ? e.message : t.agents.errorCreateEnrollment);
    } finally {
      setEnrollmentLoading(false);
    }
  }, [t]);

  const openAdd = () => {
    setForm(emptyForm);
    setEditingKey("new");
    setServerUrl(defaultServerUrl());
    setInstallNetwork("lan");
    setAgentPort(9101);
    setCopied(false);
    void issueEnrollment();
  };
  const openEdit = async (hostKey: string) => {
    // Fetch full host config (HostSummary doesn't include config fields)
    try {
      const found = await getHostConfig(hostKey);
      setForm(hostToForm(found));
      setEditingKey(hostKey);
      setFormError(null);
    } catch {
      setFormError(t.agents.errorLoadHost);
    }
  };
  const closeForm = () => {
    setEditingKey(null);
    setFormError(null);
    setEnrollment(null);
  };

  const handleSave = useCallback(async () => {
    if (!form.host_key.trim()) {
      setFormError(t.agents.errorHostKeyRequired);
      return;
    }
    if (!form.display_name.trim()) {
      setFormError(t.agents.errorDisplayNameRequired);
      return;
    }

    setSaving(true);
    setFormError(null);
    try {
      if (editingKey === "new") {
        await createHost({
          host_key: form.host_key.trim(),
          display_name: form.display_name.trim(),
          scrape_interval_secs: form.scrape_interval_secs,
          load_threshold: form.load_threshold,
          ports: parsePorts(form.ports),
          containers: parseContainers(form.containers),
        });
      } else if (editingKey) {
        await updateHost(editingKey, {
          display_name: form.display_name.trim(),
          scrape_interval_secs: form.scrape_interval_secs,
          load_threshold: form.load_threshold,
          ports: parsePorts(form.ports),
          containers: parseContainers(form.containers),
        });
      }
      await mutate();
      closeForm();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : t.agents.errorSaveFailed);
    } finally {
      setSaving(false);
    }
  }, [form, editingKey, mutate, t]);

  const handleDelete = useCallback(async (hostKey: string) => {
    try {
      await deleteHost(hostKey);
      removeHost(hostKey);
      await mutate();
      setDeleteConfirm(null);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.agents.errorDeleteFailed);
    }
  }, [mutate, removeHost, t]);

  const updateField = <K extends keyof HostFormData>(key: K, value: HostFormData[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const normalizedServerUrl = serverUrl.trim().replace(/\/+$/, "");
  const installCommand = enrollment
    ? [
      "curl -fsSL https://raw.githubusercontent.com/sounmu/netsentinel/main/scripts/install-agent.sh \\",
      "  | sudo bash -s -- \\",
      `      --server-url ${shellArg(normalizedServerUrl)} \\`,
      `      --enroll-token ${shellArg(enrollment.token)} \\`,
      `      --network ${installNetwork} \\`,
      `      --port ${agentPort}`,
    ].join("\n")
    : "";

  const copyInstallCommand = async () => {
    if (!installCommand) return;
    try {
      await navigator.clipboard.writeText(installCommand);
      setCopied(true);
      toast.success(t.agents.copied);
    } catch {
      toast.error(t.agents.errorCopyFailed);
    }
  };

  return (
    <div className="page-content fade-in">
      <PageHeader
        icon={<Settings size={16} aria-hidden="true" />}
        title={t.agents.title}
        badge={hosts?.length ?? 0}
        description={t.agents.description}
        right={
          editingKey === null ? (
            <Button variant="primary" onClick={openAdd}>
              <Plus size={14} aria-hidden="true" /> {t.agents.addAgent}
            </Button>
          ) : undefined
        }
      />

      {/* Add/Edit form */}
      {editingKey !== null && (
        <Panel>
          <PanelHeader
            title={editingKey === "new" ? t.agents.addAgentTitle : t.agents.editAgentTitle}
            right={
              <Button variant="ghost" size="sm" icon onClick={closeForm} aria-label={t.agents.cancel}>
                <X size={14} aria-hidden="true" />
              </Button>
            }
          />

          <div className="form-body">
            {editingKey === "new" && (
              <section className="install-block">
                <div className="install-block__head">
                  <span className="install-block__title">
                    <Terminal size={14} aria-hidden="true" />
                    {t.agents.installCommand}
                  </span>
                  <Button variant="secondary" size="sm" onClick={issueEnrollment} disabled={enrollmentLoading}>
                    <RefreshCw size={13} aria-hidden="true" /> {t.agents.newToken}
                  </Button>
                </div>

                <div className="form-grid form-grid--tight">
                  <Field label={t.agents.serverUrl} htmlFor="agent-server-url">
                    <input id="agent-server-url" className="date-input"
                      value={serverUrl}
                      onChange={(e) => setServerUrl(e.target.value)} />
                  </Field>
                  <Field label={t.agents.installPort} htmlFor="agent-install-port">
                    <input id="agent-install-port" className="date-input" type="number" min={1} max={65535}
                      value={agentPort}
                      onChange={(e) => setAgentPort(parseInt(e.target.value, 10) || 9101)} />
                  </Field>
                  <Field label={t.agents.network} htmlFor="agent-network">
                    <div id="agent-network" className="segmented segmented--fill">
                      {(["lan", "tailscale"] as InstallNetwork[]).map((mode) => (
                        <button
                          key={mode}
                          type="button"
                          role="tab"
                          aria-selected={installNetwork === mode}
                          className="segmented__item"
                          onClick={() => setInstallNetwork(mode)}
                        >
                          {mode === "lan" ? t.agents.networkLan : t.agents.networkTailscale}
                        </button>
                      ))}
                    </div>
                  </Field>
                </div>

                <pre className="code-block install-block__code">
                  {enrollmentLoading ? t.agents.creatingToken : installCommand}
                </pre>

                <div className="install-block__foot">
                  <span className="install-block__expiry">
                    {enrollment
                      ? t.agents.tokenExpires.replace("{time}", new Date(enrollment.expires_at).toLocaleTimeString())
                      : t.agents.tokenUnavailable}
                  </span>
                  <Button variant="primary" size="sm" onClick={copyInstallCommand}
                    disabled={!installCommand || enrollmentLoading}>
                    <Copy size={13} aria-hidden="true" /> {copied ? t.agents.copied : t.agents.copy}
                  </Button>
                </div>
              </section>
            )}

            <div className="form-grid">
              <Field label={t.agents.hostKey} required htmlFor="agent-host-key">
                {editingKey === "new" ? (
                  <input id="agent-host-key" className="date-input"
                    placeholder="192.168.1.10:9101" value={form.host_key}
                    onChange={(e) => updateField("host_key", e.target.value)} />
                ) : (
                  <div className="readonly-value">{form.host_key}</div>
                )}
              </Field>
              <Field label={t.agents.displayName} required htmlFor="agent-display-name">
                <input id="agent-display-name" className="date-input" style={{ fontFamily: "inherit" }}
                  placeholder="Production Server" value={form.display_name}
                  onChange={(e) => updateField("display_name", e.target.value)} />
              </Field>
              <Field label={t.agents.scrapeInterval} htmlFor="agent-scrape-interval">
                <input id="agent-scrape-interval" className="date-input" type="number" min={1}
                  value={form.scrape_interval_secs}
                  onChange={(e) => updateField("scrape_interval_secs", parseInt(e.target.value) || 10)} />
              </Field>
              <Field label={t.agents.loadThreshold} htmlFor="agent-load-threshold">
                <input id="agent-load-threshold" className="date-input" type="number" step="0.1"
                  value={form.load_threshold}
                  onChange={(e) => updateField("load_threshold", parseFloat(e.target.value) || 4.0)} />
              </Field>
              <Field label={t.agents.monitoredPorts} htmlFor="agent-ports">
                <input id="agent-ports" className="date-input"
                  placeholder="80, 443, 5432" value={form.ports}
                  onChange={(e) => updateField("ports", e.target.value)} />
              </Field>
              <Field label={t.agents.dockerContainers} htmlFor="agent-containers">
                <input id="agent-containers" className="date-input" style={{ fontFamily: "inherit" }}
                  placeholder="empty = monitor all" value={form.containers}
                  onChange={(e) => updateField("containers", e.target.value)} />
              </Field>
            </div>

            {formError && (
              <div className="inline-error" role="alert">
                <AlertTriangle size={14} aria-hidden="true" /> {formError}
              </div>
            )}

            <div className="form-actions">
              <Button variant="secondary" onClick={closeForm}>{t.agents.cancel}</Button>
              <Button variant="primary" onClick={handleSave} disabled={saving}>
                <Save size={13} aria-hidden="true" /> {saving ? t.agents.saving : t.agents.save}
              </Button>
            </div>
          </div>
        </Panel>
      )}

      {/* Host list */}
      <section className="page-section">
        <h2 className="section-title">{t.agents.registeredAgents}</h2>

        {isLoading && <SkeletonRows count={3} height={44} />}

        {error && (
          <Panel>
            <EmptyState tone="error" title={t.agents.errorLoadHost} />
          </Panel>
        )}

        {hosts && hosts.length === 0 && (
          <Panel>
            <EmptyState
              icon={<Server size={28} aria-hidden="true" />}
              title={t.agents.noAgents}
              description={t.agents.noAgentsHint}
              action={
                <Button variant="primary" onClick={openAdd}>
                  <Plus size={14} aria-hidden="true" /> {t.agents.addAgent}
                </Button>
              }
            />
          </Panel>
        )}

        {hosts && hosts.length > 0 && (
          <div className="row-stack">
            {hosts.map((host) => (
              <div key={host.host_key} className="record-row">
                <StatusDot tone={host.is_online ? "ok" : "off"} />
                <div className="record-row__id">
                  <span className="record-row__name">{host.display_name}</span>
                  <span className="record-row__meta">{host.host_key}</span>
                </div>
                <Badge tone={host.is_online ? "ok" : "mute"}>
                  {host.is_online ? t.common.online : t.common.offline}
                </Badge>
                <div className="record-row__actions">
                  {deleteConfirm === host.host_key ? (
                    <>
                      <Button variant="danger" size="sm" onClick={() => handleDelete(host.host_key)}>
                        {t.agents.deleteConfirmText}
                      </Button>
                      <Button variant="secondary" size="sm" onClick={() => setDeleteConfirm(null)}>
                        {t.agents.cancel}
                      </Button>
                    </>
                  ) : (
                    <>
                      <Button variant="secondary" size="sm" icon aria-label="Edit" title="Edit"
                        onClick={() => openEdit(host.host_key)}>
                        <Pencil size={13} aria-hidden="true" />
                      </Button>
                      <Button variant="danger" size="sm" icon aria-label="Delete" title="Delete"
                        onClick={() => setDeleteConfirm(host.host_key)}>
                        <Trash2 size={13} aria-hidden="true" />
                      </Button>
                    </>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
