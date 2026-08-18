"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { KeyRound } from "lucide-react";
import { toast } from "sonner";
import { ApiError, changePassword } from "@/app/lib/api";
import { useAuth } from "@/app/auth/AuthContext";
import { useI18n } from "@/app/i18n/I18nContext";
import { Button, Field, Panel, PanelHeader } from "@/app/components/ui";

/**
 * Change the signed-in account's password.
 *
 * Doubles as the forced-change screen: after
 * `netsentinel-server reset-admin-password`, the server refuses every route
 * except identity and this one, and `AuthenticatedShell` redirects here until
 * the flag clears. In that state the page drops the navigation affordances so
 * there is one way forward.
 */
export default function ChangePasswordPage() {
  const { t } = useI18n();
  const router = useRouter();
  const { user, logout } = useAuth();

  const forced = Boolean(user?.must_change_password);

  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setError(null);

    if (newPassword !== confirmPassword) {
      setError(t.auth.passwordMismatch);
      return;
    }
    if (newPassword === currentPassword) {
      setError(t.account.passwordUnchanged);
      return;
    }

    setSaving(true);
    try {
      await changePassword({
        current_password: currentPassword,
        new_password: newPassword,
      });
      toast.success(t.account.passwordChanged);

      // Changing the password revokes every session server-side, including
      // this one. Sign out explicitly so the user re-authenticates with the
      // password they just chose rather than hitting a confusing 401.
      logout();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(
          err.status === 401
            ? t.account.currentPasswordWrong
            : err.message || t.account.passwordChangeFailed,
        );
      } else if (err instanceof TypeError) {
        setError(t.auth.loginError.network);
      } else {
        setError(t.account.passwordChangeFailed);
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="page-content fade-in account-page">
      <Panel>
        <PanelHeader title={t.account.changePassword} />
        <div className="form-body">
          {forced && (
            <div className="notice notice--warn" role="status">
              <KeyRound size={14} aria-hidden="true" />
              <span>{t.account.temporaryPasswordNotice}</span>
            </div>
          )}

          <form onSubmit={handleSubmit} className="auth-form">
            <Field label={t.account.currentPassword} htmlFor="current-password">
              <input
                id="current-password"
                className="date-input"
                type="password"
                autoComplete="current-password"
                value={currentPassword}
                onChange={(e) => setCurrentPassword(e.target.value)}
                autoFocus
              />
            </Field>

            <Field
              label={t.account.newPassword}
              htmlFor="new-password"
              hint={t.auth.passwordPolicy}
            >
              <input
                id="new-password"
                className="date-input"
                type="password"
                autoComplete="new-password"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
              />
            </Field>

            <Field label={t.auth.confirmPassword} htmlFor="confirm-password">
              <input
                id="confirm-password"
                className="date-input"
                type="password"
                autoComplete="new-password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
              />
            </Field>

            {error && (
              <p className="auth-error" role="alert">
                {error}
              </p>
            )}

            <div className="form-actions">
              {!forced && (
                <Button variant="secondary" onClick={() => router.push("/")}>
                  {t.common.cancel}
                </Button>
              )}
              <Button type="submit" variant="primary" disabled={saving}>
                {saving ? t.alerts.saving : t.account.changePassword}
              </Button>
            </div>
          </form>
        </div>
      </Panel>
    </div>
  );
}
