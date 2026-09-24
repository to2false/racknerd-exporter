use racknerd_exporter::{Exporter, api::ApiClient, config::Config, router};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("racknerd-exporter: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "racknerd-exporter {}\nConfigure via RACKNERD_API_KEY and RACKNERD_API_HASH (or their _FILE variants).\nSee README.md for endpoint, server label, listen address, timeout, and caching options.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let config = Config::from_env()?;
    let address = config.listen;
    let api = ApiClient::new(config).map_err(|_| "cannot initialize HTTP client")?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("cannot bind listener: {error}"))?;
    eprintln!(
        "racknerd-exporter listening on {}",
        listener
            .local_addr()
            .map_err(|_| "cannot read listener address")?
    );
    axum::serve(listener, router(Exporter::new(api)))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|error| format!("HTTP server failed: {error}"))
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("cannot register SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
