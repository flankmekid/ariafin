mod auth;
mod impl_trait;
mod library;
pub mod models;
mod stream;

use reqwest::Client;
use crate::error::ApiError;

pub struct JellyfinClient {
    pub(crate) name: String,
    pub(crate) base_url: String,
    pub(crate) client: Client,
    pub(crate) device_id: String,
}

impl JellyfinClient {
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        let raw = base_url.into();
        let raw = raw.trim_end_matches('/');
        let base_url = if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else {
            format!("http://{raw}")
        };
        Self {
            name: name.into(),
            device_id: stable_device_id(),
            base_url,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    /// `X-Emby-Authorization` header value (token is None before login).
    pub(crate) fn auth_header(&self, token: Option<&str>) -> String {
        let token_part = token
            .map(|t| format!(", Token=\"{}\"", t))
            .unwrap_or_default();
        format!(
            "MediaBrowser Client=\"ariafin\", Device=\"CLI\", \
             DeviceId=\"{}\", Version=\"{}\"{token_part}",
            self.device_id,
            env!("CARGO_PKG_VERSION"),
        )
    }

    /// Shared status-code guard used by all request helpers.
    pub(crate) fn check_status(&self, resp: &reqwest::Response) -> Result<(), ApiError> {
        let s = resp.status();
        if s == 401 || s == 403 {
            return Err(ApiError::Auth("Token rejected by server".into()));
        }
        if !s.is_success() {
            return Err(ApiError::Http {
                status: s.as_u16(),
                body: String::new(),
            });
        }
        Ok(())
    }
}

/// Stable per-machine device ID using OS env vars — no external crate needed.
fn stable_device_id() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string());
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "machine".to_string());

    // FNV-1a hash → deterministic 16-char hex
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{host}-{user}").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("ariafin-{:016x}", h)
}
