use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),

    #[error("config version too new: {0}")]
    ConfigVersionTooNew(u32),

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("server error: {0}")]
    Server(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error("credential store error: {0}")]
    Credentials(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
