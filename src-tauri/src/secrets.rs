use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::config::{stronghold_path, DEFAULT_API_URL, DEFAULT_KEY_NAME};

const STRONGHOLD_CLIENT: &[u8] = b"lh-tsa-scan-watcher";
const STRONGHOLD_API_URL_KEY: &[u8] = b"api_url";
const STRONGHOLD_SCAN_KEY_KEY: &[u8] = b"scan_key";
const STRONGHOLD_KEY_NAME_KEY: &[u8] = b"key_name";

#[derive(Debug, Clone)]
pub struct SecretConfig {
    pub api_url: String,
    pub scan_key: String,
    pub key_name: String,
}

impl Default for SecretConfig {
    fn default() -> Self {
        Self {
            api_url: DEFAULT_API_URL.into(),
            scan_key: String::new(),
            key_name: DEFAULT_KEY_NAME.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub watch_folder: String,
    pub api_url: String,
    pub scan_key: String,
    pub key_name: String,
}

pub fn default_secret_config(scan_key: String) -> SecretConfig {
    SecretConfig {
        api_url: DEFAULT_API_URL.to_string(),
        scan_key,
        key_name: DEFAULT_KEY_NAME.to_string(),
    }
}

pub fn validate_app_config(config: &AppConfig) -> Result<(), String> {
    if config.watch_folder.trim().is_empty() {
        return Err("Missing watch folder".into());
    }
    validate_secret_config(&SecretConfig {
        api_url: config.api_url.clone(),
        scan_key: config.scan_key.clone(),
        key_name: config.key_name.clone(),
    })
}

pub fn validate_secret_config(config: &SecretConfig) -> Result<(), String> {
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

pub fn load_secret_config(password: &str) -> Result<SecretConfig, String> {
    let path = stronghold_path()?;
    if !path.exists() {
        return Ok(SecretConfig::default());
    }

    let (stronghold, client) = open_stronghold_client(password)?;
    let store = client.store();

    let config = SecretConfig {
        api_url: read_secret(&store, STRONGHOLD_API_URL_KEY)?
            .unwrap_or_else(|| DEFAULT_API_URL.into()),
        scan_key: read_secret(&store, STRONGHOLD_SCAN_KEY_KEY)?.unwrap_or_default(),
        key_name: read_secret(&store, STRONGHOLD_KEY_NAME_KEY)?
            .unwrap_or_else(|| DEFAULT_KEY_NAME.into()),
    };

    drop(stronghold);
    Ok(config)
}

pub fn save_secret_config_fresh(password: &str, config: &SecretConfig) -> Result<(), String> {
    let path = stronghold_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let stronghold = iota_stronghold::Stronghold::default();
    let client = stronghold
        .create_client(STRONGHOLD_CLIENT)
        .map_err(|error| error.to_string())?;
    let store = client.store();

    write_secret(&store, STRONGHOLD_API_URL_KEY, &config.api_url)?;
    write_secret(&store, STRONGHOLD_SCAN_KEY_KEY, &config.scan_key)?;
    write_secret(&store, STRONGHOLD_KEY_NAME_KEY, &config.key_name)?;

    let snapshot_path = iota_stronghold::SnapshotPath::from_path(&path);
    stronghold
        .commit_with_keyprovider(&snapshot_path, &key_provider(password)?)
        .map_err(|error| error.to_string())
}

fn open_stronghold_client(
    password: &str,
) -> Result<(iota_stronghold::Stronghold, iota_stronghold::Client), String> {
    let path = stronghold_path()?;
    let stronghold = iota_stronghold::Stronghold::default();
    let snapshot_path = iota_stronghold::SnapshotPath::from_path(&path);
    let provider = key_provider(password)?;

    if !path.exists() {
        return create_stronghold_client(stronghold);
    }

    stronghold
        .load_snapshot(&provider, &snapshot_path)
        .map_err(|error| format!("Không mở được kho lưu trữ bí mật: {error}"))?;

    let client = stronghold
        .load_client(STRONGHOLD_CLIENT)
        .or_else(|_| stronghold.create_client(STRONGHOLD_CLIENT))
        .map_err(|error| error.to_string())?;

    Ok((stronghold, client))
}

fn create_stronghold_client(
    stronghold: iota_stronghold::Stronghold,
) -> Result<(iota_stronghold::Stronghold, iota_stronghold::Client), String> {
    let client = stronghold
        .create_client(STRONGHOLD_CLIENT)
        .map_err(|error| error.to_string())?;
    Ok((stronghold, client))
}

fn key_provider(password: &str) -> Result<iota_stronghold::KeyProvider, String> {
    let key = Sha256::digest(password.as_bytes()).to_vec();
    iota_stronghold::KeyProvider::try_from(Zeroizing::new(key)).map_err(|error| error.to_string())
}

fn read_secret(store: &iota_stronghold::Store, key: &[u8]) -> Result<Option<String>, String> {
    store
        .get(key)
        .map_err(|error| error.to_string())?
        .map(|value| String::from_utf8(value).map_err(|error| error.to_string()))
        .transpose()
}

fn write_secret(store: &iota_stronghold::Store, key: &[u8], value: &str) -> Result<(), String> {
    store
        .insert(key.to_vec(), value.as_bytes().to_vec(), None)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
