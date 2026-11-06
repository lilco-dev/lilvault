use lilvault::{
    config::{Config, ConfigError},
    http,
    service::Service,
};
use std::{fs::File, io::Read, sync::Arc};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    if let Err(error) = run().await {
        tracing::error!(%error, "startup or server failure");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() > 1 {
        return Err("usage: lilvault [config.toml]".into());
    }
    let path = args
        .first()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "config.toml".into());
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(ConfigError("configuration must be a regular file").into());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(ConfigError("configuration exceeds 1 MiB").into());
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| ConfigError("configuration must be UTF-8"))?;
    let config = Config::parse(text)?;
    let service = Arc::new(Service::from_config(config, |key| std::env::var(key).ok())?);
    let listener = tokio::net::TcpListener::bind(service.listen).await?;
    tracing::info!(listen = %listener.local_addr()?, "lilvault REST listening; sync health is unknown");
    axum::serve(listener, http::router(service))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term.recv() => {},
    }
}
