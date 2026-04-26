use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerType {
    Jellyfin,
    Navidrome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub token: String,
    pub user_id: String,
}

/// Opaque wrapper around a streaming URL returned by the server.
#[derive(Debug, Clone)]
pub struct StreamUrl(pub String);

impl StreamUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
