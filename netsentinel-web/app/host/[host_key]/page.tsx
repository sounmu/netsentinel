import HostPageClient from "./HostPageClient";

/**
 * Static export compatibility.
 *
 * Under `output: 'export'` every dynamic route must declare the params it
 * will prerender at build time. The host detail page is driven entirely
 * by runtime data (hosts are added/removed through /api/hosts, not baked
 * into the static bundle), so we emit a single placeholder segment here
 * and let the Axum server rewrite every `/host/*` request to the HTML
 * shell produced for `/host/_spa_fallback_/index.html`. The client
 * component reads the live host_key from `usePathname()` after hydration.
 */
/**
 * The placeholder `_spa_fallback_` is the only value Next.js materialises
 * during `next build` with `output: 'export'`. Runtime requests for other
 * host_keys never reach Next.js — Axum serves the placeholder shell for
 * every `/host/*` path, and the client resolves the live key via
 * `usePathname()` inside HostPageClient.
 *
 * In dev (`next dev`, no `output: 'export'`) Next.js's default
 * `dynamicParams: true` takes over, so any host_key renders through the
 * normal dynamic route without a 404.
 */
export function generateStaticParams() {
  return [{ host_key: "_spa_fallback_" }];
}

export default function HostPage() {
  return <HostPageClient />;
}
