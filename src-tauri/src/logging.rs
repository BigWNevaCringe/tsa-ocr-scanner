use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use chrono::Local;
use serde::Serialize;
use tauri::Emitter;

use crate::config::logs_dir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatcherLog {
    pub level: String,
    pub message: String,
    pub path: Option<String>,
}

pub fn emit_log(app: &tauri::AppHandle, level: &str, message: &str, path: Option<&Path>) {
    let path_text = path.map(|path| path.display().to_string());
    let _ = append_log_file(level, message, path_text.as_deref());
    let _ = app.emit(
        "watcher-log",
        WatcherLog {
            level: level.into(),
            message: message.into(),
            path: path_text,
        },
    );
}

fn append_log_file(level: &str, message: &str, path: Option<&str>) -> Result<(), String> {
    let dir = logs_dir()?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

    let now = Local::now();
    let file_path = dir.join(format!("{}.txt", now.format("%Y-%m-%d")));
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .map_err(|error| error.to_string())?;

    let path_suffix = path.map(|path| format!(" | {path}")).unwrap_or_default();
    writeln!(
        file,
        "{} | {} | {}{}",
        now.format("%Y-%m-%d %H:%M:%S"),
        level.to_uppercase(),
        message,
        path_suffix
    )
    .map_err(|error| error.to_string())
}
