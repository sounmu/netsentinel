"use client";

import { usePathname, useRouter } from "next/navigation";
import { useEffect } from "react";
import { useAuth } from "./auth/AuthContext";
import Navbar from "./components/Navbar";
import ServiceWorkerRegistration from "./components/ServiceWorkerRegistration";
import ErrorBoundary from "./components/ErrorBoundary";
import { SSEProvider } from "./lib/sse-context";

const PASSWORD_CHANGE_PATH = "/account/password";

export function AuthenticatedShell({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  const pathname = usePathname();
  const router = useRouter();

  // After `netsentinel-server reset-admin-password`, the server rejects every
  // route except identity and the password change. Redirecting keeps the UI
  // honest about that instead of letting the user browse into a wall of 403s.
  const mustChangePassword = Boolean(user?.must_change_password);
  const onChangePage = pathname === PASSWORD_CHANGE_PATH;

  useEffect(() => {
    if (mustChangePassword && !onChangePage) {
      router.replace(PASSWORD_CHANGE_PATH);
    }
  }, [mustChangePassword, onChangePage, router]);

  return (
    <>
      <ServiceWorkerRegistration />
      <SSEProvider>
        <ErrorBoundary>
          <div className="app-layout">
            {/* The navbar links everywhere the server is currently refusing,
                so it would be an invitation to a dead end. */}
            {!mustChangePassword && <Navbar />}
            <main id="main-content" tabIndex={-1}>
              {mustChangePassword && !onChangePage ? null : children}
            </main>
          </div>
        </ErrorBoundary>
      </SSEProvider>
    </>
  );
}
