use std::path::Path;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// Mount the web static export bundle onto the given router.
///
/// The web tier used to run as a separate Next.js `output: 'standalone'`
/// Node.js server on port 3001. From v0.3.6 it is built via
/// `output: 'export'` (plain HTML + JS) and served directly by Axum so the
/// homelab deployment collapses to a single container without the ~35 MB
/// Node.js runtime.
///
/// Expected layout under `dir`:
///   - `index.html`, `agents/index.html`, `alerts/index.html`, …
///   - `host/_spa_fallback_/index.html` — SPA shell emitted by the
///     dynamic route's `generateStaticParams`. Every `/host/*` request
///     is rewritten to this file so the client can read the real
///     `host_key` from `window.location`.
///   - `404.html` — generic not-found page for unmatched paths.
///
/// After mounting:
///   - `/host/*` → `dir/host/*`, falling back to the SPA shell.
///   - Anything else not already claimed by the API → `dir/*`, falling
///     back to `404.html`.
pub fn mount(router: Router, dir: &Path) -> Router {
    let spa_shell = ServeFile::new(dir.join("host/_spa_fallback_/index.html"));
    let host_service = ServeDir::new(dir.join("host")).fallback(spa_shell);

    let not_found = ServeFile::new(dir.join("404.html"));
    let general = ServeDir::new(dir).fallback(not_found);

    router
        .nest_service("/host", host_service)
        .fallback_service(general)
}
