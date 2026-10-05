"use client";

import { useEffect } from "react";

/**
 * Root-level error boundary. This REPLACES the root layout, so
 * `globals.css` is never loaded and no design token exists at this
 * point — literal values are the correct choice here, not a violation
 * of DESIGN.md §8. They mirror the Instrument Panel palette so a
 * catastrophic failure still looks like the same product, and the
 * inline stylesheet carries both themes since there is no ThemeProvider
 * to consult.
 */
const FALLBACK_CSS = `
  .ge-root {
    --canvas: #F5F7F7; --surface: #FFFFFF; --hairline: #DFE4E4;
    --ink: #14181A; --muted: #6B7574; --solid: #1B2122; --on-solid: #FFFFFF;
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
    margin: 0;
    background: var(--canvas);
    color: var(--ink);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  @media (prefers-color-scheme: dark) {
    .ge-root {
      --canvas: #0D0F10; --surface: #15191A; --hairline: #262C2D;
      --ink: #E4E8E8; --muted: #7A8483; --solid: #DCE1E1; --on-solid: #0D0F10;
    }
  }
  .ge-card {
    max-width: 460px; width: 100%;
    padding: 40px 24px;
    border-radius: 8px;
    border: 1px solid var(--hairline);
    background: var(--surface);
    text-align: center;
  }
  .ge-title { font-size: 17px; font-weight: 600; letter-spacing: -0.016em; margin: 0 0 6px; }
  .ge-body { font-size: 12px; color: var(--muted); line-height: 1.5; margin: 0 0 20px; }
  .ge-btn {
    height: 30px; padding: 0 12px;
    border-radius: 6px; border: 1px solid transparent;
    background: var(--solid); color: var(--on-solid);
    font-family: inherit; font-size: 12px; font-weight: 600;
    cursor: pointer;
  }
`;

export default function GlobalError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error(error);
  }, [error]);

  return (
    <html lang="en">
      <body>
        <style dangerouslySetInnerHTML={{ __html: FALLBACK_CSS }} />
        <div className="ge-root">
          <div className="ge-card">
            <h1 className="ge-title">Application error</h1>
            <p className="ge-body">
              NetSentinel could not recover from a root-level rendering failure.
            </p>
            <button type="button" onClick={reset} className="ge-btn">
              Retry
            </button>
          </div>
        </div>
      </body>
    </html>
  );
}
