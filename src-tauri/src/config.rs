use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_API_URL: &str = "https://tsa-ocr.lhu.edu.vn/api/v1/scan-machines/profile-image";
pub const DEFAULT_HEALTH_URL: &str = "https://tsa-ocr.lhu.edu.vn/api/v1/scan-machines/health";
pub const DEFAULT_KEY_NAME: &str = "ocr";
pub const CONFIG_PASSWORD: &str = "1233979";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct FolderConfig {
    pub watch_folder: String,
    pub has_scan_key: bool,
}

impl Default for FolderConfig {
    fn default() -> Self {
        Self {
            watch_folder: String::new(),
            has_scan_key: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicConfig {
    pub watch_folder: String,
    pub has_scan_key: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConfigRequest {
    pub watch_folder: String,
    pub scan_key: String,
    pub password: String,
}

pub fn load_folder_config() -> Result<FolderConfig, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(FolderConfig::default());
    }

    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

pub fn save_folder_config(config: &FolderConfig) -> Result<(), String> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let content = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

pub fn config_path() -> Result<PathBuf, String> {
    app_config_dir()
        .map(|path| path.join("config.json"))
        .ok_or_else(|| "Cannot resolve config directory".into())
}

pub fn stronghold_path() -> Result<PathBuf, String> {
    app_config_dir()
        .map(|path| path.join("secrets.stronghold"))
        .ok_or_else(|| "Cannot resolve config directory".into())
}

pub fn logs_dir() -> Result<PathBuf, String> {
    app_config_dir()
        .map(|path| path.join("logs"))
        .ok_or_else(|| "Cannot resolve config directory".into())
}

fn app_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join("lh-tsa-scan-watcher"))
}
