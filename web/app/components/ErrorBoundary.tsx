"use client";

import React from "react";
import { AlertTriangle } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import { Button } from "@/app/components/ui";

interface ErrorBoundaryProps {
  children: React.ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

/** Functional fallback — isolated so it can use hooks (`useI18n`) while the
 *  class boundary below owns the React error-handling lifecycle. */
function ErrorFallback({
  error,
  onReload,
}: {
  error: Error | null;
  onReload: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="boundary-fallback">
      <AlertTriangle size={28} aria-hidden="true" className="boundary-fallback__icon" />
      <h2 className="boundary-fallback__title">{t.errorBoundary.title}</h2>
      <p className="boundary-fallback__message">
        {error?.message || t.errorBoundary.fallbackMessage}
      </p>
      <Button variant="primary" onClick={onReload}>
        {t.errorBoundary.reload}
      </Button>
    </div>
  );
}

export default class ErrorBoundary extends React.Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  private handleReload = () => {
    this.setState({ hasError: false, error: null });
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <ErrorFallback error={this.state.error} onReload={this.handleReload} />
      );
    }
    return this.props.children;
  }
}
