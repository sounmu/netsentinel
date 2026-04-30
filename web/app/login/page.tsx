"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Shield } from "lucide-react";
import { toast } from "sonner";
import { useAuth } from "@/app/auth/AuthContext";
import { useI18n } from "@/app/i18n/I18nContext";
import { ApiError, startGoogleOAuth } from "@/app/lib/api";

export default function LoginPage() {
  const auth = useAuth();
  const { t } = useI18n();
  const router = useRouter();
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (auth.user) {
      router.replace("/");
    }
  }, [auth.user, router]);

  useEffect(() => {
    const error = new URLSearchParams(window.location.search).get("error");
    if (error === "not_allowed") {
      toast.error(t.auth.loginError.notAllowed);
    } else if (error === "rate_limited") {
      toast.error(t.auth.loginError.rateLimit);
    } else if (error === "oauth") {
      toast.error(t.auth.loginError.oauth);
    }
  }, [t]);

  if (auth.user) {
    return null;
  }

  const handleGoogleLogin = async () => {
    setLoading(true);
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
        background:
          "linear-gradient(180deg, var(--bg-primary) 0%, var(--bg-secondary) 100%)",
      }}
    >
      <section
        className="glass-card"
        style={{
          maxWidth: 420,
          width: "100%",
          padding: 32,
          borderRadius: 8,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            marginBottom: 10,
            justifyContent: "center",
          }}
        >
          <Shield size={28} style={{ color: "var(--accent-blue)" }} />
          <h1 style={{ color: "var(--text-primary)", fontSize: 24, margin: 0 }}>
            NetSentinel
          </h1>
        </div>
        <p
          style={{
            color: "var(--text-muted)",
            fontSize: 14,
            lineHeight: 1.5,
            margin: "0 0 24px",
            textAlign: "center",
          }}
        >
          {t.auth.googleDescription}
        </p>
        <button
          type="button"
          onClick={handleGoogleLogin}
          disabled={loading}
          style={{
            width: "100%",
            minHeight: 44,
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
            cursor: loading ? "not-allowed" : "pointer",
            opacity: loading ? 0.7 : 1,
          }}
        >
          <span aria-hidden="true" style={{ fontWeight: 700, fontSize: 18 }}>
            G
          </span>
          {loading ? "..." : t.auth.signInWithGoogle}
        </button>
      </section>
    </main>
  );
}
