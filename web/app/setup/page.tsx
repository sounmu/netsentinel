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
    <main
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: 20,
      }}
    >
      <section className="glass-card" style={{ maxWidth: 420, width: "100%", padding: 32 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, justifyContent: "center", marginBottom: 10 }}>
          <Shield size={28} style={{ color: "var(--accent-blue)" }} />
          <h1 style={{ color: "var(--text-primary)", fontSize: 24, margin: 0 }}>
            {t.auth.setupTitle}
          </h1>
        </div>
        <p style={{ color: "var(--text-muted)", textAlign: "center", margin: "0 0 24px", fontSize: 14 }}>
          {t.auth.setupDescription}
        </p>

        <form onSubmit={handleSubmit}>
          <label htmlFor="setup-username" style={{ display: "block", color: "var(--text-muted)", marginBottom: 6, fontSize: 14 }}>
            {t.auth.username}
          </label>
          <input
            id="setup-username"
            className="date-input"
            type="text"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            style={{ width: "100%", boxSizing: "border-box", marginBottom: 16 }}
            autoFocus
          />

          <label htmlFor="setup-password" style={{ display: "block", color: "var(--text-muted)", marginBottom: 6, fontSize: 14 }}>
            {t.auth.password}
          </label>
          <input
            id="setup-password"
            className="date-input"
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            style={{ width: "100%", boxSizing: "border-box", marginBottom: 16 }}
          />

          <label htmlFor="setup-confirm" style={{ display: "block", color: "var(--text-muted)", marginBottom: 6, fontSize: 14 }}>
            {t.auth.confirmPassword}
          </label>
          <input
            id="setup-confirm"
            className="date-input"
            type="password"
            value={confirmPassword}
            onChange={(event) => setConfirmPassword(event.target.value)}
            style={{ width: "100%", boxSizing: "border-box", marginBottom: 16 }}
          />

          {error && (
            <p style={{ color: "var(--danger)", fontSize: 13, margin: "0 0 16px" }}>
              {error}
            </p>
          )}

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
            {loading ? "..." : t.auth.setupButton}
          </button>
        </form>
      </section>
    </main>
  );
}
