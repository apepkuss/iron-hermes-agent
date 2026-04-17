#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;

use tracing::info;

use iron_server::{init_tracing, spawn_server};

fn main() {
    init_tracing();

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    let port = rt.block_on(async {
        let port = spawn_server("127.0.0.1:0")
            .await
            .expect("failed to start iron-hermes server");
        info!("iron-hermes server started on http://127.0.0.1:{port}");
        port
    });

    std::thread::spawn(move || {
        rt.block_on(std::future::pending::<()>());
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let url = format!("http://127.0.0.1:{port}");

            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url.parse().unwrap()),
            )
            .title("Iron Hermes")
            .inner_size(1024.0, 768.0)
            .min_inner_size(480.0, 600.0)
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
