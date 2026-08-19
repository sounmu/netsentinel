"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import useSWR from "swr";
import { Shield, Terminal } from "lucide-react";
import { toast } from "sonner";
import { useAuth } from "@/app/auth/AuthContext";
import { useI18n } from "@/app/i18n/I18nContext";
import {
  ApiError,
  AuthStatus,
  getAuthStatusUrl,
  login as apiLogin,
  publicFetcher,
  startGoogleOAuth,
} from "@/app/lib/api";
import { Button, Field } from "@/app/components/ui";

export default function LoginPage() {
  const auth = useAuth();
  const { t } = useI18n();
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [googleLoading, setGoogleLoading] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);

  const { data: authStatus } = useSWR<AuthStatus>(
    getAuthStatusUrl(),
    publicFetcher,
    { revalidateOnFocus: false, dedupingInterval: 60_000 },
  );

  useEffect(() => {
    if (authStatus?.setup_required) {
      router.replace("/setup");
    }
  }, [authStatus, router]);

  useEffect(() => {
    if (auth.user) {
      router.replace("/");
    }
  }, [auth.user, router]);

  useEffect(() => {
    const error = new URLSearchParams(window.location.search).get("error");
    if (error === "not_allowed") {
      toast.error(t.auth.loginError.notAllowed);
    } else if (error === "not_linked") {
      toast.error(t.auth.loginError.notLinked);
    } else if (error === "oauth_conflict") {
      toast.error(t.auth.loginError.oauthConflict);
    } else if (error === "rate_limited") {
      toast.error(t.auth.loginError.rateLimit);
    } else if (error === "oauth") {
      toast.error(t.auth.loginError.oauth);
    }
  }, [t]);

  if (authStatus?.setup_required || auth.user) {
    return null;
  }

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!username.trim()) {
      toast.error(t.auth.usernameRequired);
      return;
    }

    setLoading(true);
    try {
      const response = await apiLogin(username, password);
      auth.login(response.token, response.user);
      router.replace("/");
    } catch (err) {
      if (err instanceof ApiError) {
        toast.error(err.status === 429 ? t.auth.loginError.rateLimit : t.auth.loginError.invalid);
      } else if (err instanceof TypeError) {
        toast.error(t.auth.loginError.network);
      } else {
        toast.error(t.auth.loginError.generic);
      }
    } finally {
      setLoading(false);
    }
  };

  const handleGoogleLogin = async () => {
    setGoogleLoading(true);
    try {
      const response = await startGoogleOAuth();
      window.location.assign(response.authorize_url);
    } catch (err) {
      if (err instanceof ApiError && err.status === 429) {
        toast.error(t.auth.loginError.rateLimit);
      } else if (err instanceof TypeError) {
        toast.error(t.auth.loginError.network);
      } else {
        toast.error(t.auth.loginError.generic);
      }
      setGoogleLoading(false);
    }
  };

  return (
    <main className="auth-shell">
      <section className="auth-card">
        <div className="auth-card__head">
          <Shield size={22} aria-hidden="true" />
          <h1 className="auth-card__title">{t.auth.login}</h1>
        </div>

        <form onSubmit={handleSubmit} className="auth-form">
          <Field label={t.auth.username} htmlFor="login-username">
            <input
              id="login-username"
              className="date-input"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoFocus
            />
          </Field>

          <Field label={t.auth.password} htmlFor="login-password">
            <input
              id="login-password"
              className="date-input"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </Field>

          <Button type="submit" variant="primary" disabled={loading} className="auth-submit">
            {loading ? t.common.loading : t.auth.loginButton}
          </Button>
        </form>

        {/* Recovery is deliberately not self-service. A reset button here
            would let anyone who can reach this page take over the instance,
            which for a single-admin tool is total compromise. Running a
            command on the host proves ownership; this just says how. */}
        <div className="auth-recovery">
          <button
            type="button"
            className="auth-recovery__toggle"
            onClick={() => setShowRecovery((v) => !v)}
            aria-expanded={showRecovery}
          >
            {t.auth.forgotPassword}
          </button>

          {showRecovery && (
            <div className="auth-recovery__body">
              <p className="auth-recovery__lede">
                <Terminal size={13} aria-hidden="true" />
                {t.auth.forgotPasswordHelp}
              </p>
              <pre className="code-block">netsentinel-server reset-admin-password</pre>
              <p className="auth-recovery__note">{t.auth.forgotPasswordNote}</p>
            </div>
          )}
        </div>

        {authStatus?.oauth_enabled && (
          <>
            <div className="auth-divider">
              <span>{t.auth.or}</span>
            </div>

            <Button
              variant="secondary"
              onClick={handleGoogleLogin}
              disabled={googleLoading}
              className="auth-submit"
            >
              <span aria-hidden="true" className="auth-google-mark">G</span>
              {googleLoading ? t.common.loading : t.auth.signInWithGoogle}
            </Button>
          </>
        )}
      </section>
    </main>
  );
}
