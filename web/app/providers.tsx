"use client";

import dynamic from "next/dynamic";
import { usePathname } from "next/navigation";
import { I18nProvider } from "./i18n/I18nContext";
import { ThemeProvider } from "./theme/ThemeContext";
import { AuthProvider } from "./auth/AuthContext";

const AuthenticatedShell = dynamic(
  () => import("./authenticated-shell").then((mod) => mod.AuthenticatedShell),
  { ssr: false },
);

const ClientToaster = dynamic(
  () => import("./components/ClientToaster"),
  { ssr: false },
);

const PUBLIC_PATHS = ["/login", "/setup", "/status"];

function RouteShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const isPublic = PUBLIC_PATHS.some(
    (path) => pathname === path || pathname.startsWith(path + "/"),
  );

  if (isPublic) {
    return <main id="main-content" tabIndex={-1}>{children}</main>;
  }

  return <AuthenticatedShell>{children}</AuthenticatedShell>;
}

/**
 * All client-side providers in a single component, so the root `layout.tsx`
 * can stay a Server Component.
 *
 * Previous layout inlined the deep `ThemeProvider → I18nProvider →
 * AuthProvider → SSEProvider → ErrorBoundary → Navbar` tree directly in a
 * file without `"use client"`, which meant Next.js inferred `RootLayout`
 * as a client boundary and every page below it lost the option to be
 * server-rendered. Pulling the tree behind this barrier restores the
 * server/client split. The authenticated app shell is dynamically loaded
 * only for protected routes so public pages do not pay the navbar/SSE/service
 * worker cost in their initial route chunk.
 */
export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <ThemeProvider>
      <I18nProvider>
        <AuthProvider>
          <ClientToaster />
          <RouteShell>{children}</RouteShell>
        </AuthProvider>
      </I18nProvider>
    </ThemeProvider>
  );
}
