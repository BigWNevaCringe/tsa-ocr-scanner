use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, RecvTimeoutError, Sender},
        Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    watch_folder: String,
    api_url: String,
    scan_key: String,
    key_name: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            watch_folder: String::new(),
            api_url: "https://tsa-ocr.lhu.edu.vn/api/v1/scan-machines/profile-image".into(),
            scan_key: String::new(),
            key_name: "ocr".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WatcherLog {
    level: String,
    message: String,
    path: Option<String>,
}

struct WatcherHandle {
    stop_tx: Sender<()>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct RuntimeState {
    watcher: Option<WatcherHandle>,
}

struct AppState {
    runtime: Mutex<RuntimeState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(RuntimeState::default()),
        }
    }
}

#[tauri::command]
fn load_config() -> Result<AppConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    validate_config(&config)?;
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let content = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

#[tauri::command]
fn watcher_status(state: tauri::State<AppState>) -> Result<bool, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    Ok(runtime.watcher.is_some())
}

#[tauri::command]
fn start_watcher(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    validate_config(&config)?;
    save_config(config.clone())?;

    let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    if runtime.watcher.is_some() {
        return Ok(());
    }

    let watch_path = PathBuf::from(&config.watch_folder);
    if !watch_path.exists() {
        return Err("Watch folder not found".into());
    }

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        emit_log(&app, "info", "Watcher started", Some(&watch_path));

        let (event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = match RecommendedWatcher::new(
            move |result| {
                let _ = event_tx.send(result);
            },
            Config::default(),
        ) {
            Ok(watcher) => watcher,
            Err(error) => {
                emit_log(&app, "error", &format!("Watcher init failed: {error}"), None);
                return;
            }
        };

        if let Err(error) = watcher.watch(&watch_path, RecursiveMode::Recursive) {
            emit_log(&app, "error", &format!("Watch folder failed: {error}"), Some(&watch_path));
            return;
        }

        let mut recent_uploads: HashMap<PathBuf, Instant> = HashMap::new();

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            match event_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => handle_event(&app, &config, event, &mut recent_uploads),
                Ok(Err(error)) => emit_log(&app, "error", &format!("Watcher event failed: {error}"), None),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        emit_log(&app, "info", "Watcher stopped", None);
    });

    runtime.watcher = Some(WatcherHandle { stop_tx, join: handle });
    Ok(())
}

#[tauri::command]
fn stop_watcher(state: tauri::State<AppState>) -> Result<(), String> {
    let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    if let Some(handle) = runtime.watcher.take() {
        let _ = handle.stop_tx.send(());
        let _ = handle.join.join();
    }
    Ok(())
}

fn validate_config(config: &AppConfig) -> Result<(), String> {
    if config.watch_folder.trim().is_empty() {
        return Err("Missing watch folder".into());
    }
    if config.api_url.trim().is_empty() {
        return Err("Missing API URL".into());
    }
    if config.scan_key.trim().is_empty() {
        return Err("Missing scan key".into());
    }
    if config.key_name.trim().is_empty() {
        return Err("Missing key name".into());
    }
    Ok(())
}

fn config_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|path| path.join("lh-tsa-scan-watcher").join("config.json"))
        .ok_or_else(|| "Cannot resolve config directory".into())
}

fn handle_event(
    app: &tauri::AppHandle,
    config: &AppConfig,
    event: Event,
    recent_uploads: &mut HashMap<PathBuf, Instant>,
) {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return;
    }

    for path in event.paths {
        if path.is_file() && is_image(&path) {
            let now = Instant::now();
            if recent_uploads
                .get(&path)
                .is_some_and(|last_seen| now.duration_since(*last_seen) < Duration::from_secs(10))
            {
                continue;
            }

            recent_uploads.insert(path.clone(), now);
            recent_uploads.retain(|_, last_seen| now.duration_since(*last_seen) < Duration::from_secs(60));
            upload_image(app, config, path);
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"))
        .unwrap_or(false)
}

fn wait_until_file_ready(path: &Path) -> Result<(), String> {
    let mut last_size = 0;

    for _ in 0..30 {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        let size = metadata.len();
        if size > 0 && size == last_size {
            return Ok(());
        }
        last_size = size;
        thread::sleep(Duration::from_secs(1));
    }

    Err("File not ready after 30 seconds".into())
}

fn upload_image(app: &tauri::AppHandle, config: &AppConfig, path: PathBuf) {
    emit_log(app, "info", "New image detected", Some(&path));

    if let Err(error) = wait_until_file_ready(&path) {
        emit_log(app, "error", &format!("File wait failed: {error}"), Some(&path));
        return;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scan-image")
        .to_string();
    let mime = mime_guess::from_path(&path).first_or_octet_stream().to_string();

    let file_part = match multipart::Part::file(&path) {
        Ok(part) => match part.file_name(file_name).mime_str(&mime) {
            Ok(part) => part,
            Err(error) => {
                emit_log(app, "error", &format!("MIME type failed: {error}"), Some(&path));
                return;
            }
        },
        Err(error) => {
            emit_log(app, "error", &format!("Read file failed: {error}"), Some(&path));
            return;
        }
    };

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("key_name", config.key_name.clone());

    let result = Client::new()
        .post(&config.api_url)
        .header("Authorization", format!("ScanKey {}", config.scan_key))
        .multipart(form)
        .timeout(Duration::from_secs(120))
        .send();

    match result {
        Ok(response) if response.status().is_success() => {
            emit_log(app, "success", "Upload success", Some(&path));
        }
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            emit_log(app, "error", &format!("Upload failed: HTTP {status} {body}"), Some(&path));
        }
        Err(error) => emit_log(app, "error", &format!("Upload request failed: {error}"), Some(&path)),
    }
}

fn emit_log(app: &tauri::AppHandle, level: &str, message: &str, path: Option<&Path>) {
    let _ = app.emit(
        "watcher-log",
        WatcherLog {
            level: level.into(),
            message: message.into(),
            path: path.map(|path| path.display().to_string()),
        },
    );
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            watcher_status,
            start_watcher,
            stop_watcher
        ])
        .setup(|app| {
            let state = app.state::<AppState>();
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            runtime.watcher = None;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
