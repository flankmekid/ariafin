use serde::Deserialize;
use std::collections::HashMap;

/// Top-level paginated response from /Items, /Artists, etc.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinPage<T> {
    pub items: Vec<T>,
    pub total_record_count: u32,
}

/// `{Id, Name}` pair returned in `AlbumArtists` / `ArtistItems` arrays.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NameIdPair {
    pub id: String,
}

/// Generic item DTO returned by most library endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    pub sort_name: Option<String>,

    // Artist-specific
    pub child_count: Option<u32>,

    // Album-specific
    pub album_artist: Option<String>,
    /// Parsed from the `AlbumArtists` array (the reliable field).
    #[serde(default)]
    pub album_artists: Vec<NameIdPair>,
    /// Legacy fallback — some Jellyfin versions return this as a flat array.
    #[serde(default)]
    pub album_artist_ids: Vec<String>,
    pub production_year: Option<u16>,
    #[serde(default)]
    pub genres: Vec<String>,

    // Track-specific
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub index_number: Option<u16>,
    pub parent_index_number: Option<u8>,
    pub run_time_ticks: Option<i64>,
    pub container: Option<String>,
    pub bit_rate: Option<u32>,

    // Shared
    #[serde(default)]
    pub image_tags: HashMap<String, String>,
    pub user_data: Option<JellyfinUserData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinUserData {
    pub play_count: Option<u32>,
    pub is_favorite: Option<bool>,
}

/// POST /Users/AuthenticateByName response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub user: JellyfinUser,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinUser {
    pub id: String,
    pub name: String,
}

/// Convert Jellyfin run_time_ticks (100-nanosecond units) to whole seconds.
pub fn ticks_to_secs(ticks: i64) -> u32 {
    (ticks / 10_000_000) as u32
}
