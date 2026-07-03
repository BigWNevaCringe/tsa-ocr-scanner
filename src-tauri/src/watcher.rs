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

use crate::{logging::emit_log, secrets::AppConfig};

struct WatcherHandle {
    stop_tx: Sender<()>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct RuntimeState {
    watcher: Option<WatcherHandle>,
}

pub struct AppState {
    runtime: Mutex<RuntimeState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(RuntimeState::default()),
        }
    }
}

pub fn watcher_status(state: &AppState) -> Result<bool, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    Ok(runtime.watcher.is_some())
}

pub fn start_watcher(
    app: tauri::AppHandle,
    state: &AppState,
    config: AppConfig,
) -> Result<(), String> {
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
                emit_log(
                    &app,
                    "error",
                    &format!("Watcher init failed: {error}"),
                    None,
                );
                return;
            }
        };

        if let Err(error) = watcher.watch(&watch_path, RecursiveMode::Recursive) {
            emit_log(
                &app,
                "error",
                &format!("Watch folder failed: {error}"),
                Some(&watch_path),
            );
            return;
        }

        let mut recent_uploads: HashMap<PathBuf, Instant> = HashMap::new();

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            match event_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => handle_event(&app, &config, event, &mut recent_uploads),
                Ok(Err(error)) => emit_log(
                    &app,
                    "error",
                    &format!("Watcher event failed: {error}"),
                    None,
                ),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        emit_log(&app, "info", "Watcher stopped", None);
    });

    runtime.watcher = Some(WatcherHandle {
        stop_tx,
        join: handle,
    });
    Ok(())
}

pub async fn stop_watcher(state: &AppState) -> Result<(), String> {
    let handle = {
        let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
        runtime.watcher.take()
    };

    if let Some(handle) = handle {
        let _ = handle.stop_tx.send(());
        tauri::async_runtime::spawn_blocking(move || {
            let _ = handle.join.join();
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(())
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
        if path.is_file() && is_image(&path) && !is_processed_path(&path) {
            let now = Instant::now();
            if recent_uploads
                .get(&path)
                .is_some_and(|last_seen| now.duration_since(*last_seen) < Duration::from_secs(10))
            {
                continue;
            }

            recent_uploads.insert(path.clone(), now);
            recent_uploads
                .retain(|_, last_seen| now.duration_since(*last_seen) < Duration::from_secs(60));
            upload_image(app, config, path);
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        })
        .unwrap_or(false)
}

fn is_processed_path(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|name| matches!(name, "done" | "failed"))
            .unwrap_or(false)
    })
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
        emit_log(
            app,
            "error",
            &format!("File wait failed: {error}"),
            Some(&path),
        );
        move_processed_file(app, &path, "failed");
        return;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scan-image")
        .to_string();
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();

    let file_part = match multipart::Part::file(&path) {
        Ok(part) => match part.file_name(file_name).mime_str(&mime) {
            Ok(part) => part,
            Err(error) => {
                emit_log(
                    app,
                    "error",
                    &format!("MIME type failed: {error}"),
                    Some(&path),
                );
                move_processed_file(app, &path, "failed");
                return;
            }
        },
        Err(error) => {
            emit_log(
                app,
                "error",
                &format!("Read file failed: {error}"),
                Some(&path),
            );
            move_processed_file(app, &path, "failed");
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
            move_processed_file(app, &path, "done");
        }
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            emit_log(
                app,
                "error",
                &format!("Upload failed: HTTP {status} {body}"),
                Some(&path),
            );
            move_processed_file(app, &path, "failed");
        }
        Err(error) => {
            emit_log(
                app,
                "error",
                &format!("Upload request failed: {error}"),
                Some(&path),
            );
            move_processed_file(app, &path, "failed");
        }
    }
}

fn move_processed_file(app: &tauri::AppHandle, path: &Path, folder_name: &str) {
    match processed_destination(path, folder_name) {
        Ok(destination) => {
            if let Some(parent) = destination.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    emit_log(
                        app,
                        "error",
                        &format!("Create {folder_name} folder failed: {error}"),
                        Some(path),
                    );
                    return;
                }
            }

            match fs::rename(path, &destination) {
                Ok(()) => emit_log(
                    app,
                    "info",
                    &format!("Moved to {folder_name}"),
                    Some(&destination),
                ),
                Err(error) => emit_log(
                    app,
                    "error",
                    &format!("Move to {folder_name} failed: {error}"),
                    Some(path),
                ),
            }
        }
        Err(error) => emit_log(app, "error", &error, Some(path)),
    }
}

fn processed_destination(path: &Path, folder_name: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Cannot resolve image folder".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "Cannot resolve image filename".to_string())?;
    let target_dir = parent.join(folder_name);
    let mut destination = target_dir.join(file_name);

    if !destination.exists() {
        return Ok(destination);
    }

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let extension = path.extension().and_then(|value| value.to_str());

    for index in 1..1000 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        destination = target_dir.join(candidate_name);
        if !destination.exists() {
            return Ok(destination);
        }
    }

    Err("Cannot find available processed filename".into())
}
