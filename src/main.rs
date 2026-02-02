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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("threader=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    cli.run().await
}
