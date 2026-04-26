use serde::{Deserialize, Serialize};
use crate::types::Album;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistId(pub String);

impl std::fmt::Display for ArtistId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub sort_name: Option<String>,
    pub album_count: u32,
    pub cover_art_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
}
