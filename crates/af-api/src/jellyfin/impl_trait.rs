use async_trait::async_trait;
use af_core::types::{
    Album, AlbumDetail, AlbumFilter, AlbumId, Artist, ArtistDetail, ArtistId,
    AuthToken, CoverArtId, LyricsData, Playlist, PlaylistId, StreamUrl,
    Track, TrackFilter, TrackId,
};
use crate::{
    error::ApiError,
    server_trait::{MusicServer, SearchResults},
};
use super::JellyfinClient;

#[async_trait]
impl MusicServer for JellyfinClient {
    fn server_name(&self) -> &str { &self.name }
    fn base_url(&self) -> &str { &self.base_url }

    async fn authenticate(&self, username: &str, password: &str)
        -> Result<AuthToken, ApiError>
    {
        self.do_authenticate(username, password).await
    }

    async fn validate_token(&self, token: &AuthToken) -> Result<bool, ApiError> {
        self.do_validate_token(token).await
    }

    async fn get_artists(&self, token: &AuthToken) -> Result<Vec<Artist>, ApiError> {
        self.fetch_artists(token).await
    }

    async fn get_artist(&self, token: &AuthToken, id: &ArtistId)
        -> Result<ArtistDetail, ApiError>
    {
        let artist_url = format!(
            "{}/Artists/{}?UserId={}",
            self.base_url, id.0, token.user_id,
        );
        let resp = self
            .client
            .get(&artist_url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let item: super::models::JellyfinItem = resp
            .json()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))?;

        let artist = super::library::item_to_artist(item);

        let albums = self
            .fetch_albums(token, AlbumFilter { artist_id: Some(id.clone()), ..Default::default() })
            .await?;

        Ok(ArtistDetail { artist, albums })
    }

    async fn get_albums(&self, token: &AuthToken, filter: AlbumFilter)
        -> Result<Vec<Album>, ApiError>
    {
        self.fetch_albums(token, filter).await
    }

    async fn get_album(&self, token: &AuthToken, id: &AlbumId)
        -> Result<AlbumDetail, ApiError>
    {
        let albums = self
            .fetch_albums(token, AlbumFilter::default())
            .await?;
        let album = albums
            .into_iter()
            .find(|a| &a.id == id)
            .ok_or_else(|| ApiError::NotFound(format!("album {}", id.0)))?;

        let tracks = self
            .fetch_tracks(token, TrackFilter {
                album_id: Some(id.clone()),
                ..Default::default()
            })
            .await?;

        Ok(AlbumDetail { album, tracks })
    }

    async fn get_tracks(&self, token: &AuthToken, filter: TrackFilter)
        -> Result<Vec<Track>, ApiError>
    {
        self.fetch_tracks(token, filter).await
    }

    async fn get_playlists(&self, token: &AuthToken)
        -> Result<Vec<Playlist>, ApiError>
    {
        self.fetch_playlists(token).await
    }

    async fn get_playlist_tracks(
        &self, token: &AuthToken, id: &PlaylistId,
    ) -> Result<Vec<Track>, ApiError> {
        self.fetch_playlist_tracks(token, id).await
    }

    async fn get_recently_played(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Track>, ApiError>
    {
        self.fetch_recently_played(token, limit).await
    }

    async fn get_recently_added(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Album>, ApiError>
    {
        self.fetch_recently_added(token, limit).await
    }

    async fn get_most_played(&self, token: &AuthToken, limit: u32)
        -> Result<Vec<Track>, ApiError>
    {
        self.fetch_most_played(token, limit).await
    }

    async fn get_favorites(&self, token: &AuthToken)
        -> Result<Vec<Track>, ApiError>
    {
        self.fetch_favorites(token).await
    }

    async fn get_stream_url(
        &self, token: &AuthToken, id: &TrackId, max_bitrate_kbps: Option<u32>,
    ) -> Result<StreamUrl, ApiError> {
        Ok(self.build_stream_url(token, id, max_bitrate_kbps))
    }

    async fn get_cover_art_url(
        &self, token: &AuthToken, id: &CoverArtId, size: u32,
    ) -> Result<String, ApiError> {
        Ok(self.build_cover_art_url(token, id, size))
    }

    async fn get_lyrics(&self, token: &AuthToken, id: &TrackId)
        -> Result<Option<LyricsData>, ApiError>
    {
        self.fetch_lyrics(token, id).await
    }

    async fn report_playback_start(
        &self, token: &AuthToken, id: &TrackId,
    ) -> Result<(), ApiError> {
        self.report_start(token, id).await
    }

    async fn report_playback_stop(
        &self, token: &AuthToken, id: &TrackId, position_secs: f64,
    ) -> Result<(), ApiError> {
        self.report_stop(token, id, position_secs).await
    }

    async fn set_favorite(
        &self, token: &AuthToken, id: &TrackId, favorite: bool,
    ) -> Result<(), ApiError> {
        self.do_set_favorite(token, id, favorite).await
    }

    async fn search(
        &self, token: &AuthToken, query: &str, limit: u32,
    ) -> Result<SearchResults, ApiError> {
        let (artists, albums, tracks) = self.do_search(token, query, limit).await?;
        Ok(SearchResults { artists, albums, tracks })
    }
}
