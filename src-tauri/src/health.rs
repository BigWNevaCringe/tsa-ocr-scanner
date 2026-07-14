use std::time::Duration;

use reqwest::{blocking::Client, StatusCode};
use serde::Deserialize;

use crate::{
    config::DEFAULT_HEALTH_URL,
    secrets::{validate_secret_config, SecretConfig},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    data: HealthData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthData {
    is_active: bool,
}

pub fn check_api_key_health(config: &SecretConfig) -> Result<(), String> {
    validate_secret_config(config)?;

    let response = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Không tạo được HTTP client kiểm tra scan key: {error}"))?
        .get(DEFAULT_HEALTH_URL)
        .header(
            "Authorization",
            format!("ScanKey {}", config.scan_key.trim()),
        )
        .send();

    match response {
        Ok(response) if response.status().is_success() => {
            let health = response
                .json::<HealthResponse>()
                .map_err(|error| format!("Không đọc được phản hồi kiểm tra scan key: {error}"))?;

            if health.data.is_active {
                Ok(())
            } else {
                Err(invalid_key_message())
            }
        }
        Ok(response)
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) =>
        {
            Err(invalid_key_message())
        }
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            Err(format!(
                "Không kiểm tra được scan key: HTTP {status} {body}"
            ))
        }
        Err(error) if error.is_timeout() => {
            Err("Không kiểm tra được scan key: server timeout".into())
        }
        Err(error) if error.is_connect() => Err(format!(
            "Không kiểm tra được scan key: không kết nối được server ({error})"
        )),
        Err(error) => Err(format!("Không kiểm tra được scan key: {error}")),
    }
}

fn invalid_key_message() -> String {
    "Scan key không hợp lệ, đã hết hạn hoặc đã bị thu hồi".into()
}
