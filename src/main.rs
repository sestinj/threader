mod agents;
mod auth;
mod cli;
mod daemon;
mod hooks;
mod storage;
mod sync;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use sentry::IntoDsn;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Init Sentry for error reporting. Disabled when THREADER_NO_TELEMETRY=1.
    // The guard must live for the lifetime of main so events flush on shutdown.
    let telemetry_disabled = std::env::var("THREADER_NO_TELEMETRY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let _sentry_guard = sentry::init(sentry::ClientOptions {
        dsn: if telemetry_disabled {
            None
        } else {
            "https://7492a78bfa30c5b54f76adaa5b129e36@o4505462064283648.ingest.us.sentry.io/4510818480619520".into_dsn().ok().flatten()
        },
        release: sentry::release_name!(),
        traces_sample_rate: 0.2,
        ..Default::default()
    });

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("threader=info".parse()?),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(sentry::integrations::tracing::layer())
        .init();

    let cli = Cli::parse();
    cli.run().await
}
