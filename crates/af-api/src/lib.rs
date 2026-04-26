pub mod error;
pub mod jellyfin;
pub mod server_trait;

pub use error::ApiError;
pub use jellyfin::JellyfinClient;
pub use server_trait::MusicServer;
