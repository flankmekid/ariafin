use serde::{Deserialize, Serialize};
use crate::types::{AlbumId, ArtistId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(pub String);

impl std::fmt::Display for TrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CoverArtId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub sort_title: Option<String>,
    pub album_id: Option<AlbumId>,
    pub album_title: Option<String>,
    pub artist_id: Option<ArtistId>,
    pub artist_name: Option<String>,
    pub disc_number: Option<u8>,
    pub track_number: Option<u16>,
    pub duration_secs: Option<u32>,
    pub bitrate: Option<u32>,
    pub format: Option<String>,
    pub cover_art_id: Option<CoverArtId>,
    pub has_lyrics: bool,
    pub play_count: u32,
    pub last_played_at: Option<i64>,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TrackFilter {
    pub album_id: Option<AlbumId>,
    pub artist_id: Option<ArtistId>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LyricsData {
    pub synced: bool,
    pub lines: Vec<LyricsLine>,
}

#[derive(Debug, Clone)]
pub struct LyricsLine {
    pub timestamp_ms: Option<u32>,
    pub text: String,
}
