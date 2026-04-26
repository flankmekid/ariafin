use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("server returned {status}: {body}")]
    Http { status: u16, body: String },

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("response parse error: {0}")]
    Parse(String),

    #[error("{0}")]
    Other(String),
}

impl ApiError {
    pub fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_) | Self::Http { status: 401, .. })
    }
}
