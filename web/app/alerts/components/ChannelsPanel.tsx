"use client";

import type React from "react";
import { useEffect, useState } from "react";
import useSWR from "swr";
import { toast } from "sonner";
import {
  Bot,
  Mail,
  MessageCircle,
  Plus,
  Save,
  Send,
  Trash2,
  UsersRound,
  Webhook,
} from "lucide-react";
import {
  getNotificationChannelsUrl,
  fetcher,
  createNotificationChannel,
  updateNotificationChannel,
  deleteNotificationChannel,
  testNotificationChannel,
} from "@/app/lib/api";
import type { NotificationChannel, NotificationChannelType } from "@/app/lib/api";
import { useI18n } from "@/app/i18n/I18nContext";
import type { Translations } from "@/app/i18n/translations";
import { Switch } from "@/app/components/Switch";
import { apiErrorMessage } from "./shared";

interface Props {
  onCountChange?: (count: number | null) => void;
}

interface ChannelField {
  key: string;
  label: string;
  type?: "text" | "password" | "number";
  placeholder?: string;
}

interface ChannelOption {
  type: NotificationChannelType;
  label: string;
  description: string;
  fields: ChannelField[];
  icon: React.ReactNode;
}

export function ChannelsPanel({ onCountChange }: Props) {
  const { t } = useI18n();
  const { data: channels, mutate } = useSWR<NotificationChannel[]>(
    getNotificationChannelsUrl(),
    fetcher,
    { revalidateOnFocus: false },
  );

  const [showForm, setShowForm] = useState(false);
  const [formType, setFormType] = useState<NotificationChannelType>("discord");
  const [formName, setFormName] = useState("");
  const [formConfig, setFormConfig] = useState<Record<string, string>>({});
  const [testMsg, setTestMsg] = useState<Record<number, string>>({});

  useEffect(() => {
    onCountChange?.(channels?.length ?? null);
  }, [channels, onCountChange]);

  const channelOptions = getChannelOptions(t);
  const selectedOption =
    channelOptions.find((option) => option.type === formType) ?? channelOptions[0];

  const handleCreate = async () => {
    if (!formName.trim()) return;
    try {
      await createNotificationChannel({
        name: formName,
        channel_type: formType,
        config: normalizeConfig(formConfig),
      });
      setShowForm(false);
      setFormName("");
      setFormConfig({});
      await mutate();
    } catch (e) {
      toast.error(apiErrorMessage(e, t, t.notifications.testFailed));
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await deleteNotificationChannel(id);
      await mutate();
    } catch (e) {
      toast.error(apiErrorMessage(e, t, t.notifications.testFailed));
    }
  };

  const handleToggle = async (ch: NotificationChannel) => {
    try {
      await updateNotificationChannel(ch.id, { enabled: !ch.enabled });
      await mutate();
    } catch (e) {
      toast.error(apiErrorMessage(e, t));
    }
  };

  const handleTest = async (id: number) => {
    try {
      await testNotificationChannel(id);
      setTestMsg((prev) => ({ ...prev, [id]: t.notifications.testSuccess }));
    } catch (e) {
      setTestMsg((prev) => ({
        ...prev,
        [id]: apiErrorMessage(e, t, t.notifications.testFailed),
      }));
    }
    setTimeout(() => {
      setTestMsg((prev) => {
        const n = { ...prev };
        delete n[id];
        return n;
      });
    }, 3000);
  };

  return (
    <div
      className="alerts-panel"
      id="alerts-panel-channels"
      role="tabpanel"
      aria-labelledby="alerts-tab-channels"
    >
      <div className="alerts-section-heading">
        <div>
          <h2 className="alerts-section-title" style={{ marginBottom: 0 }}>
            {t.notifications.title}
          </h2>
          <p className="alerts-section-description">{t.notifications.description}</p>
        </div>
        <button
          type="button"
          onClick={() => setShowForm((v) => !v)}
          className="alerts-btn alerts-btn--sm alerts-btn--filled"
        >
          <Plus size={14} aria-hidden="true" />
          {t.notifications.addChannel}
        </button>
      </div>

      {showForm && (
        <div className="alerts-channel-compose" style={{ marginBottom: 12 }}>
          <div className="alerts-channel-type-grid" aria-label={t.notifications.channelType}>
            {channelOptions.map((option) => (
              <button
                key={option.type}
                type="button"
                className={`alerts-channel-type ${
                  option.type === formType ? "alerts-channel-type--selected" : ""
                }`}
                onClick={() => {
                  setFormType(option.type);
                  setFormConfig({});
                }}
                aria-pressed={option.type === formType}
              >
                <span className="alerts-channel-type__icon" aria-hidden="true">
                  {option.icon}
                </span>
                <span className="alerts-channel-type__copy">
                  <span className="alerts-channel-type__label">{option.label}</span>
                  <span className="alerts-channel-type__description">{option.description}</span>
                </span>
              </button>
            ))}
          </div>

          <div className="alerts-form-grid-2">
            <div>
              <label htmlFor="notif-channel-name" className="alerts-field__label">
                {t.notifications.channelName}
              </label>
              <input
                id="notif-channel-name"
                className="alerts-field__input"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder={selectedOption.label}
              />
            </div>
          </div>

          <div className="alerts-form-grid-auto">
            {selectedOption.fields.map((field) => (
              <div key={field.key}>
                <label htmlFor={`notif-${field.key}`} className="alerts-field__label">
                  {field.label}
                </label>
                <input
                  id={`notif-${field.key}`}
                  className="alerts-field__input"
                  type={field.type ?? "text"}
                  value={formConfig[field.key] ?? ""}
                  placeholder={field.placeholder}
                  onChange={(e) =>
                    setFormConfig((prev) => ({ ...prev, [field.key]: e.target.value }))
                  }
                />
              </div>
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
        <div key={ch.id} className="alerts-channel-card" style={{ marginBottom: 8 }}>
          <span className="alerts-channel-card__icon" aria-hidden="true">
            {channelIcon(ch.channel_type)}
          </span>
          <div className="alerts-row__grow">
            <div className="alerts-channel__name">{ch.name}</div>
            <div className="alerts-channel__type">
              {channelLabel(ch.channel_type)} · {channelEndpoint(ch)}
            </div>
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
          <span className={`alerts-channel-status ${ch.enabled ? "alerts-channel-status--on" : ""}`}>
            {ch.enabled ? t.notifications.enabled : t.notifications.disabled}
          </span>
          <Switch checked={ch.enabled} onChange={() => handleToggle(ch)} aria-label={ch.name} />
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
        <div className="alerts-empty alerts-empty--compact">
          <span className="alerts-empty__icon" aria-hidden="true">
            <Send size={24} />
          </span>
          <span className="alerts-empty__title">{t.notifications.noChannels}</span>
          <span className="alerts-empty__description">{t.notifications.noChannelsDescription}</span>
        </div>
      )}
    </div>
  );
}

function getChannelOptions(t: Translations): ChannelOption[] {
  return [
    {
      type: "discord",
      label: "Discord",
      description: t.notifications.discordDescription,
      icon: <MessageCircle size={18} />,
      fields: [
        {
          key: "webhook_url",
          label: t.notifications.webhookUrl,
          placeholder: "https://discord.com/api/webhooks/...",
        },
      ],
    },
    {
      type: "slack",
      label: "Slack",
      description: t.notifications.slackDescription,
      icon: <MessageCircle size={18} />,
      fields: [
        {
          key: "webhook_url",
          label: t.notifications.webhookUrl,
          placeholder: "https://hooks.slack.com/services/...",
        },
      ],
    },
    {
      type: "email",
      label: "Email",
      description: t.notifications.emailDescription,
      icon: <Mail size={18} />,
      fields: [
        { key: "smtp_host", label: t.notifications.smtpHost, placeholder: "smtp.example.com" },
        { key: "smtp_port", label: t.notifications.smtpPort, type: "number", placeholder: "587" },
        { key: "smtp_user", label: t.notifications.smtpUser, placeholder: "noreply@example.com" },
        { key: "smtp_pass", label: t.notifications.smtpPass, type: "password" },
        { key: "from", label: t.notifications.emailFrom, placeholder: "noreply@example.com" },
        { key: "to", label: t.notifications.emailTo, placeholder: "ops@example.com" },
      ],
    },
    {
      type: "teams",
      label: "Microsoft Teams",
      description: t.notifications.teamsDescription,
      icon: <UsersRound size={18} />,
      fields: [
        { key: "webhook_url", label: t.notifications.webhookUrl, placeholder: "https://..." },
      ],
    },
    {
      type: "telegram",
      label: "Telegram",
      description: t.notifications.telegramDescription,
      icon: <Bot size={18} />,
      fields: [
        {
          key: "bot_token",
          label: t.notifications.botToken,
          type: "password",
          placeholder: "123456:ABC...",
        },
        { key: "chat_id", label: t.notifications.chatId, placeholder: "-1001234567890" },
      ],
    },
    {
      type: "webhook",
      label: "Generic Webhook",
      description: t.notifications.webhookDescription,
      icon: <Webhook size={18} />,
      fields: [
        {
          key: "webhook_url",
          label: t.notifications.webhookUrl,
          placeholder: "https://example.com/netsentinel",
        },
      ],
    },
  ];
}

function normalizeConfig(config: Record<string, string>): Record<string, unknown> {
  const next: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(config)) {
    next[key] = key === "smtp_port" && value ? Number(value) : value;
  }
  return next;
}

function channelLabel(type: NotificationChannelType): string {
  const labels: Record<NotificationChannelType, string> = {
    discord: "Discord",
    slack: "Slack",
    email: "Email",
    teams: "Microsoft Teams",
    telegram: "Telegram",
    webhook: "Generic Webhook",
  };
  return labels[type];
}

function channelIcon(type: NotificationChannelType): React.ReactNode {
  if (type === "email") return <Mail size={18} />;
  if (type === "teams") return <UsersRound size={18} />;
  if (type === "telegram") return <Bot size={18} />;
  if (type === "webhook") return <Webhook size={18} />;
  return <MessageCircle size={18} />;
}

function channelEndpoint(channel: NotificationChannel): string {
  if (channel.channel_type === "email") {
    return String(channel.config.to ?? channel.config.smtp_host ?? "");
  }
  if (channel.channel_type === "telegram") {
    return String(channel.config.chat_id ?? "");
  }
  const url = String(channel.config.webhook_url ?? "");
  return url.length > 42 ? `${url.slice(0, 20)}...${url.slice(-14)}` : url;
}
