use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(8);

#[derive(Serialize, Clone)]
pub struct UpdaterAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct DownloadProgress {
    pub downloaded: usize,
    pub total: Option<u64>,
}

fn platform_availability() -> UpdaterAvailability {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_none() {
            return UpdaterAvailability {
                available: false,
                reason: Some(
                    "当前版本通过系统包管理器安装（如 deb）。自动更新仅支持 AppImage 版本；\
                     请执行 sudo apt upgrade 或下载 AppImage 版本使用自动更新。"
                        .into(),
                ),
            };
        }
    }
    UpdaterAvailability {
        available: true,
        reason: None,
    }
}

#[tauri::command]
pub async fn get_updater_availability() -> UpdaterAvailability {
    platform_availability()
}

#[tauri::command]
pub async fn check_for_update<R: Runtime>(app: AppHandle<R>) -> Result<Option<UpdateInfo>, String> {
    let availability = platform_availability();
    if !availability.available {
        return Err(availability
            .reason
            .unwrap_or_else(|| "Updater unavailable".into()));
    }

    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
            pub_date: update.date.map(|d| d.to_string()),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let availability = platform_availability();
    if !availability.available {
        return Err(availability
            .reason
            .unwrap_or_else(|| "Updater unavailable".into()));
    }

    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Err("没有可用更新".into());
    };

    let progress_app = app.clone();
    update
        .download_and_install(
            move |downloaded, total| {
                let _ =
                    progress_app.emit("update-progress", DownloadProgress { downloaded, total });
            },
            || {
                info!("update download finished");
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    info!("update installed, restarting app");
    app.restart();
}

pub fn spawn_startup_check<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_CHECK_DELAY).await;

        let availability = platform_availability();
        if !availability.available {
            info!(reason = ?availability.reason, "updater disabled for this install type");
            return;
        }

        let updater = match app.updater() {
            Ok(u) => u,
            Err(e) => {
                warn!(error = %e, "failed to access updater");
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                info!(version = %update.version, "update available");
                let info = UpdateInfo {
                    version: update.version.clone(),
                    current_version: update.current_version.clone(),
                    notes: update.body.clone(),
                    pub_date: update.date.map(|d| d.to_string()),
                };
                let _ = app.emit("update-available", info);
            }
            Ok(None) => {
                info!("no update available");
            }
            Err(e) => {
                warn!(error = %e, "update check failed");
            }
        }
    });
}
