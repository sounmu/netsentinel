"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import useSWR from "swr";
import { Shield } from "lucide-react";
import { useAuth } from "@/app/auth/AuthContext";
import { useI18n } from "@/app/i18n/I18nContext";
import {
  AuthStatus,
  getAuthStatusUrl,
  publicFetcher,
  setupAdmin,
} from "@/app/lib/api";
import { Button, Field } from "@/app/components/ui";

export default function SetupPage() {
  const auth = useAuth();
  const { t } = useI18n();
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const { data: authStatus } = useSWR<AuthStatus>(
    getAuthStatusUrl(),
    publicFetcher,
    { revalidateOnFocus: false },
  );

  useEffect(() => {
    if (authStatus && !authStatus.setup_required) {
      router.replace("/login");
    }
  }, [authStatus, router]);

  if (authStatus && !authStatus.setup_required) {
    return null;
  }

  const validate = () => {
    if (!username.trim()) return t.auth.usernameRequired;
    if (password.length < 8 || password.length > 128) return t.auth.passwordTooShort;
    if (!/[A-Z]/.test(password) || !/[a-z]/.test(password) || !/\d/.test(password) || !/[^A-Za-z0-9]/.test(password)) {
      return t.auth.passwordPolicy;
    }
    if (password !== confirmPassword) return t.auth.passwordMismatch;
    return null;
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const validationError = validate();
    if (validationError) {
      setError(validationError);
      return;
    }

    setError(null);
    setLoading(true);
    try {
      const response = await setupAdmin(username.trim(), password);
      auth.login(response.token, response.user);
      router.replace("/");
    } catch (err) {
      setError(err instanceof Error ? err.message : t.auth.setupFailed);
    } finally {
      setLoading(false);
    }
  };

  return (
    <main className="auth-shell">
      <section className="auth-card">
        <div className="auth-card__head">
          <Shield size={22} aria-hidden="true" />
          <h1 className="auth-card__title">{t.auth.setupTitle}</h1>
        </div>
        <p className="auth-card__lede">{t.auth.setupDescription}</p>

        <form onSubmit={handleSubmit} className="auth-form">
          <Field label={t.auth.username} htmlFor="setup-username">
            <input
              id="setup-username"
              className="date-input"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoFocus
            />
          </Field>

          <Field label={t.auth.password} htmlFor="setup-password">
            <input
              id="setup-password"
              className="date-input"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </Field>

          <Field label={t.auth.confirmPassword} htmlFor="setup-confirm">
            <input
              id="setup-confirm"
              className="date-input"
              type="password"
              value={confirmPassword}
              onChange={(event) => setConfirmPassword(event.target.value)}
            />
          </Field>

          {error && (
            <p className="auth-error" role="alert">{error}</p>
          )}

          <Button type="submit" variant="primary" disabled={loading} className="auth-submit">
            {loading ? t.common.loading : t.auth.setupButton}
          </Button>
        </form>
      </section>
    </main>
  );
}
