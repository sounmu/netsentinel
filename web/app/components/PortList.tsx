"use client";

import { PortStatus } from "@/app/types/metrics";
import { Network } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import { EmptyState, StatusDot } from "@/app/components/ui";

interface PortListProps {
  ports: PortStatus[];
}

const PORT_LABELS: Record<number, string> = {
  22: "SSH",
  80: "HTTP",
  443: "HTTPS",
  3000: "App",
  3306: "MySQL",
  5432: "PostgreSQL",
  6379: "Redis",
  8080: "HTTP Alt",
  8443: "HTTPS Alt",
  9100: "Node Exporter",
  27017: "MongoDB",
};

export default function PortList({ ports }: PortListProps) {
  const { t } = useI18n();
  if (ports.length === 0) {
    return (
      <EmptyState
        icon={<Network size={24} aria-hidden="true" />}
        title={t.portList.noData}
      />
    );
  }

  return (
    <div className="port-list">
      {ports.map((p) => {
        const label = PORT_LABELS[p.port];
        return (
          <span key={p.port} className="port-chip" data-open={p.is_open}>
            <StatusDot tone={p.is_open ? "ok" : "crit"} />
            <span className="port-chip__num">{p.port}</span>
            {label && <span className="port-chip__label">{label}</span>}
          </span>
        );
      })}
    </div>
  );
}
