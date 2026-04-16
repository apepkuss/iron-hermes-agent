use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tracing::info;

use iron_server::config::IronConfig;
use iron_server::routes::chat::chat_completions;
use iron_server::routes::config_api::{get_config, list_toolsets, update_config};
use iron_server::routes::health::health;
use iron_server::routes::models::{list_models, list_provider_models};
use iron_server::routes::models_status::models_status;
use iron_server::routes::session::reset_session;
use iron_server::routes::session_search::search_sessions;
use iron_server::routes::static_files;
use iron_server::state::build_app_state;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = IronConfig::load();
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let port = config.server.port;
    let state = Arc::new(build_app_state(config));

    let app = Router::new()
        .route("/", get(static_files::index))
        .route("/assets/{*path}", get(static_files::static_file))
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/provider/models", get(list_provider_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/config", get(get_config).post(update_config))
        .route("/api/toolsets", get(list_toolsets))
        .route("/api/models/status", get(models_status))
        .route("/api/session/reset", post(reset_session))
        .route("/api/sessions/search", get(search_sessions))
        .with_state(state);

    let listener = TcpListener::bind(&addr).await.unwrap();
    info!("iron-hermes server listening on http://{addr}");
    info!("Open your browser: http://localhost:{port}");
    axum::serve(listener, app).await.unwrap();
}
