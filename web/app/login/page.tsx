"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import useSWR from "swr";
import { Shield } from "lucide-react";
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

export default function LoginPage() {
  const auth = useAuth();
  const { t } = useI18n();
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);
  const [googleLoading, setGoogleLoading] = useState(false);

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
    <main
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: 20,
        background:
          "linear-gradient(180deg, var(--bg-primary) 0%, var(--bg-secondary) 100%)",
      }}
    >
      <section className="glass-card" style={{ maxWidth: 420, width: "100%", padding: 32 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            marginBottom: 24,
            justifyContent: "center",
          }}
        >
          <Shield size={28} style={{ color: "var(--accent-blue)" }} />
          <h1 style={{ color: "var(--text-primary)", fontSize: 24, margin: 0 }}>
            {t.auth.login}
          </h1>
        </div>

        <form onSubmit={handleSubmit}>
          <div style={{ marginBottom: 16 }}>
            <label htmlFor="login-username" style={{ display: "block", color: "var(--text-muted)", marginBottom: 6, fontSize: 14 }}>
              {t.auth.username}
            </label>
            <input
              id="login-username"
              className="date-input"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              style={{ width: "100%", boxSizing: "border-box" }}
              autoFocus
            />
          </div>

          <div style={{ marginBottom: 24 }}>
            <label htmlFor="login-password" style={{ display: "block", color: "var(--text-muted)", marginBottom: 6, fontSize: 14 }}>
              {t.auth.password}
            </label>
            <input
              id="login-password"
              className="date-input"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              style={{ width: "100%", boxSizing: "border-box" }}
            />
          </div>

          <button
            type="submit"
            disabled={loading}
            style={{
              width: "100%",
              minHeight: 42,
              padding: "10px 16px",
              backgroundColor: "var(--accent-blue)",
              color: "var(--text-on-accent, #fff)",
              border: "none",
              borderRadius: 8,
              fontSize: 15,
              fontWeight: 600,
              cursor: loading ? "not-allowed" : "pointer",
              opacity: loading ? 0.7 : 1,
            }}
          >
            {loading ? "..." : t.auth.loginButton}
          </button>
        </form>

        {authStatus?.oauth_enabled && (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: 12, margin: "22px 0" }}>
              <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
              <span style={{ color: "var(--text-muted)", fontSize: 12 }}>{t.auth.or}</span>
              <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
            </div>

            <button
              type="button"
              onClick={handleGoogleLogin}
              disabled={googleLoading}
              style={{
                width: "100%",
                minHeight: 42,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 10,
                padding: "10px 16px",
                backgroundColor: "var(--surface-elevated)",
                color: "var(--text-primary)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                fontSize: 15,
                fontWeight: 600,
                cursor: googleLoading ? "not-allowed" : "pointer",
                opacity: googleLoading ? 0.7 : 1,
              }}
            >
              <span aria-hidden="true" style={{ fontWeight: 700, fontSize: 18 }}>
                G
              </span>
              {googleLoading ? "..." : t.auth.signInWithGoogle}
            </button>
          </>
        )}
      </section>
    </main>
  );
}
