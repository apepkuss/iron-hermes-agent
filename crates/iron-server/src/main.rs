mod config;
mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tracing::info;

use config::ServerConfig;
use routes::chat::chat_completions;
use routes::health::health;
use routes::models::list_models;
use routes::static_files;
use state::build_app_state;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = ServerConfig::from_env();
    let addr = format!("{}:{}", config.host, config.port);
    let port = config.port;
    let state = Arc::new(build_app_state(config));

    let app = Router::new()
        .route("/", get(static_files::index))
        .route("/assets/{*path}", get(static_files::static_file))
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let listener = TcpListener::bind(&addr).await.unwrap();
    info!("iron-hermes server listening on http://{addr}");
    info!("Open your browser: http://localhost:{port}");
    axum::serve(listener, app).await.unwrap();
}
