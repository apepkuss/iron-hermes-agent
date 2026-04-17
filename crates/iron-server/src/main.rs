use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

use iron_server::config::IronConfig;
use iron_server::state::build_app_state;
use iron_server::{build_router, init_tracing};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = IronConfig::load();
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let port = config.server.port;
    let state = Arc::new(build_app_state(config));

    let app = build_router(state);

    let listener = TcpListener::bind(&addr).await.unwrap();
    info!("iron-hermes server listening on http://{addr}");
    info!("Open your browser: http://localhost:{port}");
    axum::serve(listener, app).await.unwrap();
}
