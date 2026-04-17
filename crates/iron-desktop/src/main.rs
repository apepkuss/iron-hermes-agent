#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;
mod updater;

use tracing::info;

use iron_server::config::IronConfig;
use iron_server::{init_tracing, spawn_server};

fn main() {
    init_tracing();

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    let port = rt.block_on(async {
        let config = IronConfig::load();
        let port = spawn_server(config, "127.0.0.1:0")
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            updater::get_updater_availability,
            updater::check_for_update,
            updater::install_update,
        ])
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

            tray::setup_tray(app.handle())?;
            updater::spawn_startup_check(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
