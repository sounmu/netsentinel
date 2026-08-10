import type { Metadata, Viewport } from "next";
import Script from "next/script";
import { IBM_Plex_Mono } from "next/font/google";
import localFont from "next/font/local";
import "./globals.css";
import { Providers } from "./providers";

/// Inline FOUC-killer for the dark/light theme. Runs synchronously in
/// `<head>` *before* hydration so the first paint already carries the
/// correct `data-theme` attribute. The server emits `data-theme="light"`
/// as the stable default, then React accepts this intentional pre-hydration
/// DOM update via `suppressHydrationWarning` on `<html>`. The expression is
/// kept tiny on purpose; it ships in every HTML page from the static export,
/// so size matters more than readability.
const THEME_BOOTSTRAP = `(function(){try{var t=localStorage.getItem('theme');if(t!=='dark'&&t!=='light'){t=window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';}document.documentElement.setAttribute('data-theme',t);}catch(_){}})();`;

const pretendard = localFont({
  src: "../node_modules/pretendard/dist/web/variable/woff2/PretendardVariable.woff2",
  weight: "45 920",
  style: "normal",
  variable: "--font-pretendard",
  display: "swap",
});

const ibmPlexMono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500"],
  variable: "--font-mono",
  display: "swap",
});

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#3B82F6",
};

export const metadata: Metadata = {
  title: "NetSentinel — Infrastructure Dashboard",
  description: "Real-time server infrastructure monitoring dashboard",
  appleWebApp: {
    capable: true,
    statusBarStyle: "black-translucent",
    title: "NetSentinel",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      data-theme="light"
      data-scroll-behavior="smooth"
      suppressHydrationWarning
      className={`${pretendard.variable} ${ibmPlexMono.variable}`}
    >
      <head>
        {/* `beforeInteractive` ensures the snippet runs before React hydrates,
            so the `data-theme` attribute is set before the first paint and
            no light-flash occurs on dark-mode reloads. */}
        <Script
          id="theme-bootstrap"
          strategy="beforeInteractive"
          dangerouslySetInnerHTML={{ __html: THEME_BOOTSTRAP }}
        />
      </head>
      <body>
        <a href="#main-content" className="skip-to-content">Skip to content</a>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
