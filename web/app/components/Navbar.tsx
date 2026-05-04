"use client";

import { useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import useSWR from "swr";
import {
  LayoutDashboard,
  Settings,
  Bell,
  Globe,
  Box,
  Shield,
  Link2,
  Sun,
  Moon,
  LogOut,
  Menu,
  X,
} from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";
import { useTheme } from "@/app/theme/ThemeContext";
import { useAuth } from "@/app/auth/AuthContext";
import {
  ApiError,
  AuthStatus,
  getAuthStatusUrl,
  publicFetcher,
  startGoogleOAuth,
} from "@/app/lib/api";
import { toast } from "sonner";

const HIDDEN_PATHS = ["/login", "/setup"];

export default function Navbar() {
  const pathname = usePathname();
  const router = useRouter();
  const { t, locale, setLocale } = useI18n();
  const { theme, toggleTheme } = useTheme();
  const { user, logout } = useAuth();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [linkingGoogle, setLinkingGoogle] = useState(false);
  const { data: authStatus } = useSWR<AuthStatus>(
    user ? getAuthStatusUrl() : null,
    publicFetcher,
    { revalidateOnFocus: false, dedupingInterval: 60_000 },
  );

  // Hide navbar on login/setup pages (after all hooks)
  if (HIDDEN_PATHS.includes(pathname)) return null;

  const navItems = [
    { label: t.sidebar.overview, icon: <LayoutDashboard size={15} />, href: "/", exact: true },
    { label: t.sidebar.containers, icon: <Box size={15} />, href: "/containers" },
    { label: t.sidebar.agents, icon: <Settings size={15} />, href: "/agents" },
    { label: t.sidebar.alerts, icon: <Bell size={15} />, href: "/alerts" },
    { label: t.sidebar.monitors, icon: <Globe size={15} />, href: "/monitors" },
    { label: t.sidebar.status, icon: <Shield size={15} />, href: "/status" },
  ];
  const canLinkGoogle = Boolean(user && authStatus?.oauth_enabled);

  const handleLinkGoogle = async () => {
    setLinkingGoogle(true);
    try {
      const response = await startGoogleOAuth();
      window.location.assign(response.authorize_url);
    } catch (err) {
      if (err instanceof ApiError && err.status === 429) {
        toast.error(t.auth.loginError.rateLimit);
      } else if (err instanceof TypeError) {
        toast.error(t.auth.loginError.network);
      } else {
        toast.error(t.auth.loginError.oauth);
      }
      setLinkingGoogle(false);
    }
  };

  return (
    <nav className="navbar" aria-label="Main navigation">
      {/* Logo */}
      <button
        className="navbar-logo"
        onClick={() => router.push("/")}
        style={{ border: "none", background: "none", cursor: "pointer" }}
      >
        {t.sidebar.appName}
      </button>

      {/* Desktop nav */}
      <div className="navbar-nav">
        {navItems.map((item) => {
          const active = item.exact ? pathname === item.href : pathname.startsWith(item.href);
          return (
            <button
              key={item.href}
              className={`navbar-link ${active ? "active" : ""}`}
              onClick={() => router.push(item.href)}
              aria-current={active ? "page" : undefined}
            >
              {item.icon}
              {item.label}
            </button>
          );
        })}
      </div>

      {/* Desktop actions */}
      <div className="navbar-actions">
        <button
          className="navbar-icon-btn"
          onClick={toggleTheme}
          title="Toggle theme"
          aria-label={theme === "light" ? "Switch to dark mode" : "Switch to light mode"}
        >
          {theme === "light" ? <Moon size={14} /> : <Sun size={14} />}
        </button>
        <button
          className="navbar-icon-btn"
          onClick={() => setLocale(locale === "en" ? "ko" : "en")}
          title="Toggle language"
          aria-label={locale === "en" ? "Switch to Korean" : "Switch to English"}
          style={{ fontSize: 11, fontWeight: 600, width: "auto", padding: "0 8px" }}
        >
          {locale === "en" ? "KO" : "EN"}
        </button>
        {canLinkGoogle && (
          <button
            className="navbar-icon-btn"
            onClick={handleLinkGoogle}
            disabled={linkingGoogle}
            title={t.auth.linkGoogle}
            aria-label={t.auth.linkGoogle}
            style={{
              opacity: linkingGoogle ? 0.7 : 1,
              cursor: linkingGoogle ? "not-allowed" : "pointer",
            }}
          >
            <Link2 size={14} />
          </button>
        )}
        {user && (
          <button
            className="navbar-icon-btn"
            onClick={logout}
            title={t.auth.logout}
            aria-label={t.auth.logout}
          >
            <LogOut size={14} />
          </button>
        )}
      </div>

      {/* Mobile hamburger */}
      <button
        className="navbar-mobile-toggle"
        onClick={() => setMobileOpen((v) => !v)}
        aria-label={mobileOpen ? t.sidebar.closeSidebar : t.sidebar.openSidebar}
      >
        {mobileOpen ? <X size={20} /> : <Menu size={20} />}
      </button>

      {/* Mobile dropdown menu */}
      {mobileOpen && (
        <div className="navbar-mobile-menu">
          {navItems.map((item) => {
            const active = item.exact ? pathname === item.href : pathname.startsWith(item.href);
            return (
              <button
                key={item.href}
                className={`navbar-link ${active ? "active" : ""}`}
                onClick={() => {
                  router.push(item.href);
                  setMobileOpen(false);
                }}
              >
                {item.icon}
                {item.label}
              </button>
            );
          })}
          <div style={{ borderTop: "1px solid var(--border-subtle)", margin: "4px 0", padding: "4px 0" }}>
            <div style={{ display: "flex", gap: 6, padding: "4px 14px" }}>
              <button className="navbar-icon-btn" onClick={toggleTheme} aria-label={theme === "light" ? "Switch to dark mode" : "Switch to light mode"}>
                {theme === "light" ? <Moon size={14} /> : <Sun size={14} />}
              </button>
              <button
                className="navbar-icon-btn"
                onClick={() => setLocale(locale === "en" ? "ko" : "en")}
                style={{ fontSize: 11, fontWeight: 600, width: "auto", padding: "0 8px" }}
                aria-label={locale === "en" ? "Switch to Korean" : "Switch to English"}
              >
                {locale === "en" ? "KO" : "EN"}
              </button>
              {canLinkGoogle && (
                <button
                  className="navbar-icon-btn"
                  onClick={handleLinkGoogle}
                  disabled={linkingGoogle}
                  aria-label={t.auth.linkGoogle}
                >
                  <Link2 size={14} />
                </button>
              )}
              {user && (
                <button className="navbar-icon-btn" onClick={logout} aria-label="Logout">
                  <LogOut size={14} />
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </nav>
  );
}
