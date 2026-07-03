use crate::{
    config::{
        load_folder_config, save_folder_config, FolderConfig, PublicConfig, SaveConfigRequest,
        CONFIG_PASSWORD,
    },
    secrets::{
        default_secret_config, load_secret_config, save_secret_config_fresh, validate_app_config,
        validate_secret_config, AppConfig,
    },
    watcher::{self, AppState},
};

use tauri::Manager;

#[tauri::command]
pub fn load_config() -> Result<PublicConfig, String> {
    let folder = load_folder_config()?;

    Ok(PublicConfig {
        watch_folder: folder.watch_folder,
        has_scan_key: folder.has_scan_key,
    })
}

#[tauri::command]
pub fn save_watch_folder(watch_folder: String) -> Result<(), String> {
    let mut config = load_folder_config()?;
    config.watch_folder = watch_folder;
    save_folder_config(&config)
}

#[tauri::command]
pub async fn save_config(config: SaveConfigRequest) -> Result<PublicConfig, String> {
    if config.password.trim() != CONFIG_PASSWORD {
        return Err("Sai password".into());
    }

    let config_for_secret = config.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let secrets = default_secret_config(config_for_secret.scan_key.trim().to_string());
        validate_secret_config(&secrets)?;
        save_secret_config_fresh(CONFIG_PASSWORD, &secrets)
    })
    .await
    .map_err(|error| error.to_string())??;

    save_folder_config(&FolderConfig {
        watch_folder: config.watch_folder.clone(),
        has_scan_key: true,
    })?;

    Ok(PublicConfig {
        watch_folder: config.watch_folder,
        has_scan_key: true,
    })
}

#[tauri::command]
pub fn watcher_status(state: tauri::State<AppState>) -> Result<bool, String> {
    watcher::watcher_status(&state)
}

#[tauri::command]
pub async fn start_watcher(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    watch_folder: String,
) -> Result<(), String> {
    save_watch_folder(watch_folder.clone())?;
    let secrets = tauri::async_runtime::spawn_blocking(move || load_secret_config(CONFIG_PASSWORD))
        .await
        .map_err(|error| error.to_string())??;
    let config = AppConfig {
        watch_folder,
        api_url: secrets.api_url,
        scan_key: secrets.scan_key,
        key_name: secrets.key_name,
    };
    validate_app_config(&config)?;
    watcher::start_watcher(app, &state, config)
}

#[tauri::command]
pub async fn stop_watcher(state: tauri::State<'_, AppState>) -> Result<(), String> {
    watcher::stop_watcher(&state).await
}

pub async fn auto_start_watcher(app: tauri::AppHandle) -> Result<(), String> {
    let folder = load_folder_config()?;
    if folder.watch_folder.trim().is_empty() || !folder.has_scan_key {
        return Ok(());
    }

    let secrets = tauri::async_runtime::spawn_blocking(move || load_secret_config(CONFIG_PASSWORD))
        .await
        .map_err(|error| error.to_string())??;
    let config = AppConfig {
        watch_folder: folder.watch_folder,
        api_url: secrets.api_url,
        scan_key: secrets.scan_key,
        key_name: secrets.key_name,
    };
    validate_app_config(&config)?;

    let state = app.state::<AppState>();
    watcher::start_watcher(app.clone(), &state, config)
}
