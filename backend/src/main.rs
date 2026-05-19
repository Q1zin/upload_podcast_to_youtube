use std::{env, net::SocketAddr, path::PathBuf};

use podcast_backend::{build_router, Store};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "podcast_backend=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let data_path = env::var_os("PODCAST_BACKEND_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/app_state.json"));
    let bind_addr = env::var("PODCAST_BACKEND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse::<SocketAddr>()?;

    let store = Store::load(data_path).await?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;

    tracing::info!("backend listening on http://{bind_addr}");
    axum::serve(listener, build_router(store))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
