use serde::{Deserialize, Serialize};
use crate::types::{ArtistId, Track};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlbumId(pub String);

impl std::fmt::Display for AlbumId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub sort_title: Option<String>,
    pub artist_id: Option<ArtistId>,
    pub artist_name: Option<String>,
    pub year: Option<u16>,
    pub track_count: u32,
    pub duration_secs: Option<u32>,
    pub cover_art_id: Option<String>,
    pub genre: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AlbumDetail {
    pub album: Album,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default)]
pub struct AlbumFilter {
    pub artist_id: Option<ArtistId>,
    pub genre: Option<String>,
    pub limit: Option<u32>,
}
