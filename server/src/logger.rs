use chrono::Utc;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Clone, Debug)]
struct UtcTime;

impl FormatTime for UtcTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        // Logs are emitted in UTC regardless of host timezone — no hardcoded
        // local/KST assumption. The trailing literal makes the zone explicit.
        let now = Utc::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S.%3f UTC"))
    }
}

pub fn init_tracing() -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily("logs", "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let default_level = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let console_layer = fmt::layer()
        .with_timer(UtcTime)
        .with_ansi(true)
        .with_target(false)
        .pretty();

    let file_layer = fmt::layer()
        .with_timer(UtcTime)
        .with_ansi(false)
        .with_writer(non_blocking)
        .json();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    guard
}
