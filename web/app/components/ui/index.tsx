"use client";

/**
 * Shared UI primitives — "Instrument Panel" design language.
 *
 * Before this file, every page invented its own buttons, cards, empty
 * states and form fields with inline `style={{}}` objects; the app
 * carried three parallel visual dialects as a result. Anything that
 * appears on more than one screen belongs here, styled through the
 * tokens in `globals.css` and never with raw hex or px literals.
 *
 * See DESIGN.md for the rules these components encode.
 */

import type { ReactNode } from "react";
import { meterTone, type MeterTone } from "@/app/lib/status";

/* ── Button ───────────────────────────────────────
   `primary` is ink, not a hue. With the primary action
   achromatic, the only saturated thing on a screen is
   state that needs attention.
   ─────────────────────────────────────────────── */
type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md";
  /** Square button sized for a lone icon; pass `aria-label`. */
  icon?: boolean;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon = false,
  className = "",
  type = "button",
  ...rest
}: ButtonProps) {
  const classes = [
    "btn",
    `btn--${variant}`,
    size === "sm" && "btn--sm",
    icon && "btn--icon",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return <button type={type} className={classes} {...rest} />;
}

/* ── Panel ────────────────────────────────────────
   A bordered surface. No shadow — hierarchy is carried
   by the hairline and the background step.
   ─────────────────────────────────────────────── */
export function Panel({
  children,
  className = "",
  padded = false,
}: {
  children: ReactNode;
  className?: string;
  padded?: boolean;
}) {
  return (
    <div className={`panel ${padded ? "panel--padded" : ""} ${className}`.trim()}>
      {children}
    </div>
  );
}

export function PanelHeader({
  title,
  right,
}: {
  title: string;
  right?: ReactNode;
}) {
  return (
    <div className="panel__head">
      <span className="panel__title">{title}</span>
      {right && <div className="panel__head-actions">{right}</div>}
    </div>
  );
}

/* ── Status dot ───────────────────────────────────
   `firing` is the ONLY thing in the app allowed to
   animate. A healthy fleet stays still.
   ─────────────────────────────────────────────── */
export function StatusDot({
  tone,
  firing = false,
}: {
  tone: "ok" | "warn" | "crit" | "off";
  firing?: boolean;
}) {
  const cls = { ok: "green", warn: "yellow", crit: "red", off: "grey" }[tone];
  return (
    <span
      className={`pulse-dot ${cls}`}
      {...(firing ? { "data-firing": "" } : {})}
      aria-hidden="true"
    />
  );
}

/* ── Badge ────────────────────────────────────── */
export function Badge({
  tone,
  children,
}: {
  tone: "ok" | "warn" | "crit" | "mute";
  children: ReactNode;
}) {
  const cls = {
    ok: "badge-online",
    warn: "badge-pending",
    crit: "badge-offline",
    mute: "badge-mute",
  }[tone];
  return <span className={cls}>{children}</span>;
}

/* ── Meter ────────────────────────────────────────
   Threshold colour comes from `meterTone()`, so every
   surface agrees on what a given percentage means.
   ─────────────────────────────────────────────── */
export function Meter({ value, max = 100 }: { value: number; max?: number }) {
  const pct = Math.min(Math.max((value / max) * 100, 0), 100);
  const tone: MeterTone = meterTone(pct);
  return (
    <div className="inline-meter">
      <span className="inline-meter-value">{pct.toFixed(1)}%</span>
      <span className="inline-meter-bar">
        <span className={`inline-meter-fill ${tone}`} style={{ width: `${pct}%` }} />
      </span>
    </div>
  );
}

/* ── Segmented control ────────────────────────────
   One tab language for the whole app. Replaces the
   three different tab treatments the pages each had.
   ─────────────────────────────────────────────── */
export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
  count?: number | null;
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
}: {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (next: T) => void;
  ariaLabel: string;
}) {
  return (
    <div className="segmented" role="tablist" aria-label={ariaLabel}>
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          role="tab"
          aria-selected={value === opt.value}
          className="segmented__item"
          onClick={() => onChange(opt.value)}
        >
          {opt.icon}
          {opt.label}
          {opt.count !== undefined && opt.count !== null && (
            <span className="segmented__count">{opt.count}</span>
          )}
        </button>
      ))}
    </div>
  );
}

/* ── Empty state ──────────────────────────────────
   This markup was copy-pasted into six files with
   slightly different padding and icon opacity each time.
   ─────────────────────────────────────────────── */
export function EmptyState({
  icon,
  title,
  description,
  action,
  tone = "neutral",
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  tone?: "neutral" | "error";
}) {
  return (
    <div className={`empty-state ${tone === "error" ? "empty-state--error" : ""}`.trim()}>
      {icon && <span className="empty-state__icon">{icon}</span>}
      <span className="empty-state__title">{title}</span>
      {description && <span className="empty-state__desc">{description}</span>}
      {action && <div className="empty-state__action">{action}</div>}
    </div>
  );
}

/* ── Field ────────────────────────────────────────
   Replaces the three near-identical local components
   (`FormField` in agents, `MiniField` in monitors,
   `.alerts-field__*` in alerts).
   ─────────────────────────────────────────────── */
export function Field({
  label,
  htmlFor,
  required,
  hint,
  children,
}: {
  label: string;
  htmlFor?: string;
  required?: boolean;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <label className="field__label" htmlFor={htmlFor}>
        {label}
        {required && (
          <span className="field__required" aria-hidden="true">
            *
          </span>
        )}
      </label>
      {children}
      {hint && <span className="field__hint">{hint}</span>}
    </div>
  );
}

/* ── Skeleton ─────────────────────────────────── */
export function SkeletonRows({ count = 3, height = 34 }: { count?: number; height?: number }) {
  return (
    <div className="skeleton-stack">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="skeleton" style={{ height }} />
      ))}
    </div>
  );
}

/* ── Stat tile ────────────────────────────────── */
export function StatTile({
  label,
  value,
  detail,
  tone = "neutral",
}: {
  label: string;
  value: ReactNode;
  detail?: string;
  tone?: "neutral" | "crit" | "ok";
}) {
  return (
    <div className={`stat-tile ${tone !== "neutral" ? `stat-tile--${tone}` : ""}`.trim()}>
      <span className="stat-tile__label">{label}</span>
      <span className={`stat-tile__value ${tone !== "neutral" ? `stat-tile__value--${tone}` : ""}`.trim()}>
        {value}
      </span>
      {detail && <span className="stat-tile__detail">{detail}</span>}
    </div>
  );
}
