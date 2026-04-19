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
export function generateStaticParams() {
  return [{ host_key: "_spa_fallback_" }];
}

export const dynamicParams = false;

export default function HostPage() {
  return <HostPageClient />;
}
