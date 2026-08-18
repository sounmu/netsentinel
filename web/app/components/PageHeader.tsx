"use client";

import type { ReactNode } from "react";

interface PageHeaderProps {
  /** Leading icon — kept intentionally small (16-20px lucide icon). */
  icon?: ReactNode;
  /** Page title rendered as an <h1>. */
  title: string;
  /** Optional chip rendered right after the title (usually a count). */
  badge?: ReactNode;
  /** Optional tagline under the title row. */
  description?: string;
  /** Right-aligned slot for actions, stats, or both. */
  right?: ReactNode;
  /** Center the whole block — used by the public /status hero. */
  align?: "start" | "center";
}

/**
 * Shared page header, applied on every top-level route.
 *
 * It is deliberately NOT a card. Boxing the title meant every page
 * opened with a container that held nothing but a line of text, and
 * the first real panel then read as the second item on the page. A
 * title on the canvas over a hairline reads as chrome, which is what
 * it is.
 */
export function PageHeader({
  icon,
  title,
  badge,
  description,
  right,
  align = "start",
}: PageHeaderProps) {
  return (
    <header className={`page-header page-header--align-${align}`}>
      <div className="page-header__row">
        {icon && <span className="page-header__icon">{icon}</span>}
        <h1 className="page-header__title">{title}</h1>
        {badge !== undefined && badge !== null && badge !== false && (
          <span className="page-header__badge">{badge}</span>
        )}
        {right && <div className="page-header__actions">{right}</div>}
      </div>
      {description && <p className="page-header__desc">{description}</p>}
    </header>
  );
}
