use tracing::info;

use iron_server::config::IronConfig;
use iron_server::{init_tracing, spawn_server};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = IronConfig::load();
    let addr = format!("{}:{}", config.server.host, config.server.port);

    let port = spawn_server(&addr)
        .await
        .expect("failed to start iron-hermes server");

    info!("iron-hermes server listening on http://{addr}");
    info!("Open your browser: http://localhost:{port}");

    tokio::signal::ctrl_c()
        .await
        .expect("failed to install ctrl-c handler");
}
