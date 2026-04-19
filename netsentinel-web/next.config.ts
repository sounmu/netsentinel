import type { NextConfig } from "next";

/**
 * Static export — the web tier is served by the Rust Axum server via
 * tower-http's ServeDir, embedded into the same image as the backend.
 * This drops the ~35 MB Node.js runtime that the old `output: 'standalone'`
 * configuration required and collapses the homelab deployment to a single
 * server container.
 *
 * `trailingSlash: true` makes every route emit `{route}/index.html`, which
 * is the shape ServeDir prefers (it maps `/foo/` → `/foo/index.html`
 * automatically).
 *
 * `images.unoptimized: true` disables the build-time next/image
 * optimization pipeline — required because the exported bundle has no
 * Node runtime to run the optimizer at request time.
 *
 * Local `npm run dev` still runs a full Next.js dev server on port 3001
 * against the API on port 3000, so the day-to-day authoring loop is
 * unchanged.
 */
const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
};

export default nextConfig;
