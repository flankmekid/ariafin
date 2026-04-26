use crate::types::{Album, Artist, Playlist, PlaylistId, Track, TrackId};
use std::time::Duration;

/// Events broadcast from background tasks to the TUI.
#[derive(Debug, Clone)]
pub enum BgEvent {
    // --- Authentication ---
    AuthSuccess {
        server_name: String,
        token: String,
        user_id: String,
    },
    AuthFailed(String),

    // --- Library sync ---
    SyncStarted,
    SyncProgress {
        label: String,
        done: u32,
        total: u32,
    },
    SyncComplete,
    SyncFailed(String),

    // --- Cache data ready ---
    ArtistsLoaded(Vec<Artist>),
    AlbumsLoaded(Vec<Album>),
    TracksLoaded(Vec<Track>),

    // --- Lyrics ---
    LyricsLoaded(Option<crate::types::LyricsData>),

    // --- Playlists ---
    PlaylistsLoaded(Vec<Playlist>),
    PlaylistTracksReady(Vec<Track>),

    // --- Home ---
    HomeDataLoaded {
        recently_added:  Vec<Album>,
        recently_played: Vec<Track>,
    },

    // --- Search ---
    SearchLoaded {
        artists: Vec<Artist>,
        albums:  Vec<Album>,
        tracks:  Vec<Track>,
    },

}

/// Commands sent from the TUI to background tasks.
#[derive(Debug, Clone)]
pub enum UiCommand {
    Authenticate {
        server_name: String,
        base_url: String,
        username: String,
        password: String,
    },
    StartSync {
        server_name: String,
        base_url:    String,
        token:       String,
        user_id:     String,
    },
    LoadFromCache {
        server_name: String,
    },
    ReportPlaybackStart {
        track_id: TrackId,
        base_url: String,
        token: String,
        user_id: String,
    },
    ReportPlaybackStop {
        track_id: TrackId,
        position_secs: f64,
        base_url: String,
        token: String,
        user_id: String,
    },
    FetchLyrics {
        track_id: TrackId,
        base_url: String,
        token: String,
        user_id: String,
    },
    LoadPlaylists {
        base_url: String,
        token: String,
        user_id: String,
    },
    LoadPlaylistTracks {
        playlist_id: PlaylistId,
        base_url: String,
        token: String,
        user_id: String,
    },
    FetchHomeData {
        base_url: String,
        token: String,
        user_id: String,
    },
    Search {
        query:    String,
        base_url: String,
        token:    String,
        user_id:  String,
    },
}

/// Playback commands sent from the TUI to the audio engine (Phase 3+).
#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Play { track_id: TrackId, stream_url: String },
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetVolume(u8),
    Next,
    Previous,
}

/// Events from the audio engine to the TUI (Phase 3+).
#[derive(Debug, Clone)]
pub enum AudioEvent {
    PositionChanged { position: Duration, duration: Duration },
    StateChanged { is_playing: bool },
    TrackChanged(Option<TrackId>),
    Error(String),
}
