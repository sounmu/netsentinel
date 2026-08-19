"use client";

import { useEffect, useState } from "react";
import useSWR, { useSWRConfig } from "swr";
import { toast } from "sonner";
import { Save, Trash2 } from "lucide-react";
import {
  AlertConfigRow,
  fetcher,
  getAlertConfigsUrl,
  getHostAlertConfigsUrl,
  updateHostAlertConfigs,
  deleteHostAlertConfigs,
} from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import type { HostSummary } from "@/app/types/metrics";
import type { AlertFormData, MetricPrefix } from "./shared";
import { apiErrorMessage, configsToForm, formToRequests } from "./shared";
import { MetricRuleCard } from "./MetricRuleCard";
import { Button, Drawer, SkeletonRows } from "@/app/components/ui";

interface Props {
  host: HostSummary;
  metric: MetricPrefix;
  onClose: () => void;
}

/**
 * Side-sheet editor for a single (host, metric) rule.
 * Fetches the host override, falls back to global, edits only the target
 * metric's row, and writes back all metric overrides at once
 * (the existing PUT /api/alert-configs/{host_key} contract is all-or-nothing).
 */
export function RuleDrawer({ host, metric, onClose }: Props) {
  const { t } = useI18n();
  const { mutate: mutateCache } = useSWRConfig();

  const { data: hostConfigs, mutate: mutateHost } = useSWR<AlertConfigRow[]>(
    getHostAlertConfigsUrl(host.host_key),
    fetcher,
    { revalidateOnFocus: false },
  );
  const { data: globalConfigs } = useSWR<AlertConfigRow[]>(
    getAlertConfigsUrl(),
    fetcher,
    { revalidateOnFocus: false },
  );

  const hasOverride = !!hostConfigs && hostConfigs.length > 0;
  const [form, setForm] = useState<AlertFormData | null>(null);
  const [saving, setSaving] = useState(false);

  // Seed from the host override when one exists, otherwise from the global
  // defaults — the drawer edits a copy either way, and `handleSave` writes
  // back every metric at once per the all-or-nothing PUT contract.
  useEffect(() => {
    if (hostConfigs === undefined || globalConfigs === undefined) return;
    setForm(configsToForm(hostConfigs.length > 0 ? hostConfigs : globalConfigs));
  }, [hostConfigs, globalConfigs]);

  const handleSave = async () => {
    if (!form) return;
    setSaving(true);
    try {
      await updateHostAlertConfigs(host.host_key, formToRequests(form));
      await mutateHost();
      toast.success(t.alerts.saved);
      onClose();
    } catch (e) {
      toast.error(apiErrorMessage(e, t));
    } finally {
      setSaving(false);
    }
  };

  const handleRevert = async () => {
    setSaving(true);
    try {
      await deleteHostAlertConfigs(host.host_key);
      await mutateHost(undefined, { revalidate: false });
      await mutateCache(getHostAlertConfigsUrl(host.host_key));
      toast.success(t.alerts.revertedToGlobal);
      onClose();
    } catch (e) {
      toast.error(apiErrorMessage(e, t));
    } finally {
      setSaving(false);
    }
  };

  const label = {
    cpu: t.alerts.cpuAlert,
    memory: t.alerts.memoryAlert,
    disk: t.alerts.diskAlert,
    docker: t.alerts.dockerAlert,
  }[metric];

  return (
    <Drawer
      title={label}
      subtitle={host.host_key}
      onClose={onClose}
      closeLabel={t.common.cancel}
      footer={
        <>
          {hasOverride && (
            <Button onClick={handleRevert} disabled={saving} variant="danger" size="sm">
              <Trash2 size={12} aria-hidden="true" />
              {t.alerts.deleteOverride}
            </Button>
          )}
          <Button onClick={onClose} variant="secondary" size="sm">
            {t.common.cancel}
          </Button>
          <Button onClick={handleSave} disabled={saving || !form} variant="primary" size="sm">
            <Save size={12} aria-hidden="true" />
            {saving ? t.alerts.saving : t.alerts.save}
          </Button>
        </>
      }
    >
      {form ? (
        <MetricRuleCard label={label} prefix={metric} form={form} setForm={setForm} />
      ) : (
        <SkeletonRows count={1} height={180} />
      )}
    </Drawer>
  );
}
