"use client";

import Navbar from "./components/Navbar";
import ServiceWorkerRegistration from "./components/ServiceWorkerRegistration";
import ErrorBoundary from "./components/ErrorBoundary";
import { SSEProvider } from "./lib/sse-context";

export function AuthenticatedShell({ children }: { children: React.ReactNode }) {
  return (
    <>
      <ServiceWorkerRegistration />
      <SSEProvider>
        <ErrorBoundary>
          <div className="app-layout">
            <Navbar />
            <main id="main-content" tabIndex={-1}>
              {children}
            </main>
          </div>
        </ErrorBoundary>
      </SSEProvider>
    </>
  );
}
