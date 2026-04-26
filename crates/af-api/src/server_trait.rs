use async_trait::async_trait;
use af_core::types::{
    Album, AlbumDetail, AlbumFilter, AlbumId, Artist, ArtistDetail, ArtistId,
    AuthToken, CoverArtId, LyricsData, Playlist, PlaylistId, StreamUrl,
    Track, TrackFilter, TrackId,
};
use crate::error::ApiError;

#[derive(Debug, Clone)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
}

#[async_trait]
pub trait MusicServer: Send + Sync + 'static {
    // ── Identity ──────────────────────────────────────────────────────────
    fn server_name(&self) -> &str;
    fn base_url(&self) -> &str;

    // ── Authentication ────────────────────────────────────────────────────
    async fn authenticate(&self, username: &str, password: &str)
        -> Result<AuthToken, ApiError>;
    async fn validate_token(&self, token: &AuthToken) -> Result<bool, ApiError>;

    // ── Library browsing ─────────────────────────────────────────────────
    async fn get_artists(&self, token: &AuthToken)
        -> Result<Vec<Artist>, ApiError>;
    async fn get_artist(&self, token: &AuthToken, id: &ArtistId)
        -> Result<ArtistDetail, ApiError>;
    async fn get_albums(&self, token: &AuthToken, filter: AlbumFilter)
        -> Result<Vec<Album>, ApiError>;
    async fn get_album(&self, token: &AuthToken, id: &AlbumId)
        -> Result<AlbumDetail, ApiError>;
    async fn get_tracks(&self, token: &AuthToken, filter: TrackFilter)
        -> Result<Vec<Track>, ApiError>;
    async fn get_playlists(&self, token: &AuthToken)
        -> Result<Vec<Playlist>, ApiError>;
    async fn get_playlist_tracks(
        &self, token: &AuthToken, id: &PlaylistId,
    ) -> Result<Vec<Track>, ApiError>;

    // ── Discovery / home ─────────────────────────────────────────────────
    async fn get_recently_played(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Track>, ApiError>;
    async fn get_recently_added(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Album>, ApiError>;
    async fn get_most_played(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Track>, ApiError>;
    async fn get_favorites(&self, token: &AuthToken)
        -> Result<Vec<Track>, ApiError>;

    // ── Streaming ─────────────────────────────────────────────────────────
    async fn get_stream_url(
        &self, token: &AuthToken, id: &TrackId, max_bitrate_kbps: Option<u32>,
    ) -> Result<StreamUrl, ApiError>;
    async fn get_cover_art_url(
        &self, token: &AuthToken, id: &CoverArtId, size: u32,
    ) -> Result<String, ApiError>;
    async fn get_lyrics(&self, token: &AuthToken, id: &TrackId)
        -> Result<Option<LyricsData>, ApiError>;

    // ── Playback reporting ───────────────────────────────────────────────
    async fn report_playback_start(
        &self, token: &AuthToken, id: &TrackId,
    ) -> Result<(), ApiError>;
    async fn report_playback_stop(
        &self, token: &AuthToken, id: &TrackId, position_secs: f64,
    ) -> Result<(), ApiError>;

    // ── Mutations ─────────────────────────────────────────────────────────
    async fn set_favorite(
        &self, token: &AuthToken, id: &TrackId, favorite: bool,
    ) -> Result<(), ApiError>;

    // ── Search ────────────────────────────────────────────────────────────
    async fn search(
        &self, token: &AuthToken, query: &str, limit: u32,
    ) -> Result<SearchResults, ApiError>;
}
