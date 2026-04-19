pub mod config;
pub mod routes;
pub mod state;

use std::io;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tokio::net::TcpListener;

use crate::config::IronConfig;
use crate::routes::chat::chat_completions;
use crate::routes::config_api::{get_config, list_toolsets, update_config};
use crate::routes::health::health;
use crate::routes::models::{list_models, list_provider_models};
use crate::routes::models_status::models_status;
use crate::routes::session::reset_session;
use crate::routes::session_search::search_sessions;
use crate::routes::static_files;
use crate::state::{AppState, build_app_state};

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
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
        .route("/api/sessions", get(crate::routes::sessions::list_sessions))
        .route(
            "/api/sessions/{id}",
            axum::routing::patch(crate::routes::sessions::update_session)
                .delete(crate::routes::sessions::delete_session),
        )
        .route(
            "/api/sessions/{id}/messages",
            get(crate::routes::sessions::get_session_messages),
        )
        .with_state(state)
}

pub async fn spawn_server(config: IronConfig, bind_addr: &str) -> io::Result<u16> {
    let state = Arc::new(build_app_state(config));
    let app = build_router(state);

    let listener = TcpListener::bind(bind_addr).await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("axum server exited with error: {e}");
        }
    });

    Ok(port)
}
