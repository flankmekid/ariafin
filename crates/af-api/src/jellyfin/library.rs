use af_core::types::{
    Album, AlbumFilter, AlbumId, Artist, ArtistId,
    AuthToken, CoverArtId, Playlist, PlaylistId, Track, TrackFilter, TrackId,
};
use crate::error::ApiError;
use super::{
    models::{ticks_to_secs, JellyfinItem, JellyfinPage},
    JellyfinClient,
};

const ARTIST_FIELDS: &str =
    "SortName,ChildCount,PrimaryImageAspectRatio";
const ALBUM_FIELDS: &str =
    "SortName,ChildCount,AlbumArtist,AlbumArtistIds,ProductionYear,Genres,PrimaryImageAspectRatio";
const TRACK_FIELDS: &str =
    "SortName,RunTimeTicks,IndexNumber,ParentIndexNumber,AlbumArtist,\
     AlbumArtistIds,AlbumId,Album,BitRate,Container,UserData,PrimaryImageAspectRatio";

impl JellyfinClient {
    // ── Internal helpers ──────────────────────────────────────────────────

    async fn get_items(
        &self,
        token: &AuthToken,
        item_type: &str,
        fields: &str,
        extra_params: &str,
        limit: u32,
        start_index: u32,
    ) -> Result<JellyfinPage<JellyfinItem>, ApiError> {
        let url = format!(
            "{}/Items?UserId={}&IncludeItemTypes={}&Recursive=true\
             &Fields={}&SortBy=SortName&SortOrder=Ascending\
             &Limit={}&StartIndex={}{extra_params}",
            self.base_url,
            token.user_id,
            item_type,
            fields,
            limit,
            start_index,
        );

        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;

        self.check_status(&resp)?;
        resp.json::<JellyfinPage<JellyfinItem>>()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))
    }

    /// Fetch all pages for a given item type.
    async fn get_all_items(
        &self,
        token: &AuthToken,
        item_type: &str,
        fields: &str,
        extra_params: &str,
    ) -> Result<Vec<JellyfinItem>, ApiError> {
        const PAGE: u32 = 500;
        let mut all = Vec::new();
        let mut start = 0u32;

        loop {
            let page = self
                .get_items(token, item_type, fields, extra_params, PAGE, start)
                .await?;
            let count = page.items.len() as u32;
            all.extend(page.items);
            start += count;
            if start >= page.total_record_count || count == 0 {
                break;
            }
        }

        Ok(all)
    }

    // ── Artists ───────────────────────────────────────────────────────────

    pub(crate) async fn fetch_artists(
        &self, token: &AuthToken,
    ) -> Result<Vec<Artist>, ApiError> {
        // Jellyfin has a dedicated /Artists endpoint
        let url = format!(
            "{}/Artists?UserId={}&Fields={}&SortBy=SortName&SortOrder=Ascending",
            self.base_url, token.user_id, ARTIST_FIELDS,
        );

        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;

        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> = resp
            .json()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))?;

        Ok(page.items.into_iter().map(item_to_artist).collect())
    }

    // ── Albums ────────────────────────────────────────────────────────────

    pub(crate) async fn fetch_albums(
        &self, token: &AuthToken, filter: AlbumFilter,
    ) -> Result<Vec<Album>, ApiError> {
        let artist_param = filter
            .artist_id
            .as_ref()
            .map(|id| format!("&AlbumArtistIds={}", id.0))
            .unwrap_or_default();

        let items = self
            .get_all_items(token, "MusicAlbum", ALBUM_FIELDS, &artist_param)
            .await?;

        Ok(items.into_iter().map(item_to_album).collect())
    }

    // ── Tracks ────────────────────────────────────────────────────────────

    pub(crate) async fn fetch_tracks(
        &self, token: &AuthToken, filter: TrackFilter,
    ) -> Result<Vec<Track>, ApiError> {
        let mut extra = String::new();
        if let Some(album_id) = &filter.album_id {
            extra.push_str(&format!("&ParentId={}", album_id.0));
        }
        if let Some(artist_id) = &filter.artist_id {
            extra.push_str(&format!("&ArtistIds={}", artist_id.0));
        }

        let items = self
            .get_all_items(token, "Audio", TRACK_FIELDS, &extra)
            .await?;

        Ok(items.into_iter().map(item_to_track).collect())
    }

    // ── Playlists ─────────────────────────────────────────────────────────

    pub(crate) async fn fetch_playlists(
        &self, token: &AuthToken,
    ) -> Result<Vec<Playlist>, ApiError> {
        let items = self
            .get_all_items(token, "Playlist", "SortName,ChildCount", "")
            .await?;

        Ok(items
            .into_iter()
            .map(|i| {
                let cover_art_id = cover_art_id_from_item(&i);
                Playlist {
                    id: PlaylistId(i.id),
                    name: i.name,
                    track_count: i.child_count.unwrap_or(0),
                    cover_art_id,
                }
            })
            .collect())
    }

    pub(crate) async fn fetch_playlist_tracks(
        &self, token: &AuthToken, id: &PlaylistId,
    ) -> Result<Vec<Track>, ApiError> {
        let url = format!(
            "{}/Playlists/{}/Items?UserId={}&Fields={}",
            self.base_url, id.0, token.user_id, TRACK_FIELDS,
        );

        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;

        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> = resp
            .json()
            .await
            .map_err(|e| ApiError::Parse(e.to_string()))?;

        Ok(page.items.into_iter().map(item_to_track).collect())
    }

    // ── Discovery ─────────────────────────────────────────────────────────

    pub(crate) async fn fetch_recently_played(
        &self, token: &AuthToken, limit: u32,
    ) -> Result<Vec<Track>, ApiError> {
        let url = format!(
            "{}/Items?UserId={}&SortBy=DatePlayed&SortOrder=Descending\
             &IncludeItemTypes=Audio&Recursive=true&Fields={}&Limit={}\
             &Filters=IsPlayed",
            self.base_url, token.user_id, TRACK_FIELDS, limit,
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> =
            resp.json().await.map_err(|e| ApiError::Parse(e.to_string()))?;
        Ok(page.items.into_iter().map(item_to_track).collect())
    }

    pub(crate) async fn fetch_recently_added(
        &self, token: &AuthToken, limit: u32,
    ) -> Result<Vec<Album>, ApiError> {
        let url = format!(
            "{}/Items?UserId={}&SortBy=DateCreated&SortOrder=Descending\
             &IncludeItemTypes=MusicAlbum&Recursive=true&Fields={}&Limit={}",
            self.base_url, token.user_id, ALBUM_FIELDS, limit,
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> =
            resp.json().await.map_err(|e| ApiError::Parse(e.to_string()))?;
        Ok(page.items.into_iter().map(item_to_album).collect())
    }

    pub(crate) async fn fetch_most_played(
        &self, token: &AuthToken, limit: u32,
    ) -> Result<Vec<Track>, ApiError> {
        let url = format!(
            "{}/Items?UserId={}&SortBy=PlayCount&SortOrder=Descending\
             &IncludeItemTypes=Audio&Recursive=true&Fields={}&Limit={}\
             &Filters=IsPlayed",
            self.base_url, token.user_id, TRACK_FIELDS, limit,
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> =
            resp.json().await.map_err(|e| ApiError::Parse(e.to_string()))?;
        Ok(page.items.into_iter().map(item_to_track).collect())
    }

    pub(crate) async fn fetch_favorites(
        &self, token: &AuthToken,
    ) -> Result<Vec<Track>, ApiError> {
        let url = format!(
            "{}/Items?UserId={}&Filters=IsFavorite\
             &IncludeItemTypes=Audio&Recursive=true&Fields={}",
            self.base_url, token.user_id, TRACK_FIELDS,
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> =
            resp.json().await.map_err(|e| ApiError::Parse(e.to_string()))?;
        Ok(page.items.into_iter().map(item_to_track).collect())
    }

    // ── Mutations ─────────────────────────────────────────────────────────

    pub(crate) async fn do_set_favorite(
        &self, token: &AuthToken, id: &TrackId, favorite: bool,
    ) -> Result<(), ApiError> {
        let method = if favorite { "POST" } else { "DELETE" };
        let url = format!(
            "{}/Users/{}/FavoriteItems/{}",
            self.base_url, token.user_id, id.0,
        );
        let builder = match method {
            "POST" => self.client.post(&url),
            _ => self.client.delete(&url),
        };
        let resp = builder
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        Ok(())
    }

    // ── Search ────────────────────────────────────────────────────────────

    pub(crate) async fn do_search(
        &self, token: &AuthToken, query: &str, limit: u32,
    ) -> Result<(Vec<Artist>, Vec<Album>, Vec<Track>), ApiError> {
        let encoded_query = urlencoding::encode(query);
        let url = format!(
            "{}/Items?UserId={}&SearchTerm={}&IncludeItemTypes=MusicArtist,MusicAlbum,Audio\
             &Recursive=true&Fields={}&Limit={}",
            self.base_url,
            token.user_id,
            encoded_query,
            TRACK_FIELDS,
            limit,
        );
        let resp = self
            .client
            .get(&url)
            .header("X-Emby-Authorization", self.auth_header(Some(&token.token)))
            .send()
            .await?;
        self.check_status(&resp)?;
        let page: JellyfinPage<JellyfinItem> =
            resp.json().await.map_err(|e| ApiError::Parse(e.to_string()))?;

        let mut artists = Vec::new();
        let mut albums = Vec::new();
        let mut tracks = Vec::new();

        for item in page.items {
            // Jellyfin doesn't always return Type in search; infer from structure
            if item.run_time_ticks.is_some() {
                tracks.push(item_to_track(item));
            } else if item.album_artist.is_some() || item.album_id.is_some() {
                albums.push(item_to_album(item));
            } else {
                artists.push(item_to_artist(item));
            }
        }

        Ok((artists, albums, tracks))
    }
}

// ── Conversion helpers ─────────────────────────────────────────────────────
// pub(super) so impl_trait.rs can reuse them without duplicating logic.

pub(super) fn cover_art_id_from_item(item: &JellyfinItem) -> Option<String> {
    item.image_tags.get("Primary").map(|_| item.id.clone())
}

pub(super) fn item_to_artist(item: JellyfinItem) -> Artist {
    Artist {
        cover_art_id: cover_art_id_from_item(&item),
        id: ArtistId(item.id),
        album_count: item.child_count.unwrap_or(0),
        sort_name: item.sort_name,
        name: item.name,
    }
}

fn item_to_album(item: JellyfinItem) -> Album {
    let cover_art_id = cover_art_id_from_item(&item);
    let mut artist_id_str = item.album_artists.into_iter().next().map(|a| a.id);
    if artist_id_str.is_none() {
        artist_id_str = item.album_artist_ids.into_iter().next();
    }
    Album {
        cover_art_id,
        id: AlbumId(item.id),
        artist_id: artist_id_str.map(ArtistId),
        artist_name: item.album_artist,
        year: item.production_year,
        track_count: item.child_count.unwrap_or(0),
        sort_title: item.sort_name,
        genre: item.genres.into_iter().next(),
        duration_secs: None,
        title: item.name,
    }
}

fn item_to_track(item: JellyfinItem) -> Track {
    let cover_art_id = cover_art_id_from_item(&item).map(CoverArtId);
    let ud = item.user_data.as_ref();
    let mut artist_id_str = item.album_artists.into_iter().next().map(|a| a.id);
    if artist_id_str.is_none() {
        artist_id_str = item.album_artist_ids.into_iter().next();
    }
    Track {
        cover_art_id,
        id: TrackId(item.id),
        album_id: item.album_id.map(AlbumId),
        album_title: item.album,
        artist_id: artist_id_str.map(ArtistId),
        artist_name: item.album_artist,
        track_number: item.index_number,
        disc_number: item.parent_index_number,
        duration_secs: item.run_time_ticks.map(ticks_to_secs),
        bitrate: item.bit_rate,
        format: item.container,
        sort_title: item.sort_name,
        is_favorite: ud.and_then(|u| u.is_favorite).unwrap_or(false),
        play_count: ud.and_then(|u| u.play_count).unwrap_or(0),
        has_lyrics: false,
        last_played_at: None,
        title: item.name,
    }
}
