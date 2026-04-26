use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use ratatui::widgets::ListState;

use af_core::{
    config::{loader as cfg_loader, Config},
    config::schema::TabId,
    events::{PlaybackCommand, UiCommand},
    secrets,
    types::{Album, LyricsData, Playlist, PlaybackState, RepeatMode, Track, TrackId},
};
use crate::state::{LoginModal, ServerState};

mod events;
mod render;
mod runner;

pub use runner::run;

// ── Tabs ──────────────────────────────────────────────────────────────────────

const TABS: &[&str] = &[
    "  Home  ",
    "  Artists  ",
    "  Albums  ",
    "  Songs  ",
    "  Playlists  ",
    "  Queue  ",
    "  Settings  ",
];

// ── Queue ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct QueueState {
    tracks: Vec<Track>,
    current_idx: Option<usize>,
}

impl QueueState {
    fn current_track(&self) -> Option<&Track> {
        self.current_idx.and_then(|i| self.tracks.get(i))
    }

    fn advance(&mut self, repeat: RepeatMode) -> Option<Track> {
        let idx = self.current_idx?;
        let next = match repeat {
            RepeatMode::One => idx,
            RepeatMode::All => (idx + 1) % self.tracks.len().max(1),
            RepeatMode::Off => idx + 1,
        };
        if next < self.tracks.len() {
            self.current_idx = Some(next);
            Some(self.tracks[next].clone())
        } else {
            self.current_idx = None;
            None
        }
    }

    fn go_prev(&mut self) -> Option<Track> {
        let idx = self.current_idx?;
        if idx == 0 { return None; }
        let prev = idx - 1;
        self.current_idx = Some(prev);
        Some(self.tracks[prev].clone())
    }
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct SearchState {
    query:        String,
    artists:      Vec<af_core::types::Artist>,
    albums:       Vec<Album>,
    tracks:       Vec<Track>,
    section:      usize,
    selected:     usize,
    is_searching: bool,
}

impl SearchState {
    fn has_results(&self) -> bool {
        !self.artists.is_empty() || !self.albums.is_empty() || !self.tracks.is_empty()
    }

    fn section_len(&self) -> usize {
        match self.section {
            0 => self.artists.len(),
            1 => self.albums.len(),
            _ => self.tracks.len(),
        }
    }

    fn clamp_selected(&mut self) {
        let len = self.section_len();
        if len > 0 { self.selected = self.selected.min(len - 1); }
        else { self.selected = 0; }
    }
}

// ── View states ───────────────────────────────────────────────────────────────

#[derive(Default)]
enum ArtistView {
    #[default]
    List,
    Albums { artist_name: String, albums: Vec<Album>, state: ListState },
    Tracks {
        artist_name:  String,
        albums:       Vec<Album>,
        album_idx:    usize,
        album_title:  String,
        album_artist: String,
        tracks:       Vec<Track>,
        state:        ListState,
    },
}

#[derive(Default)]
enum AlbumView {
    #[default]
    List,
    Tracks { album_title: String, album_artist: String, tracks: Vec<Track>, state: ListState },
}

#[derive(Default)]
enum PlaylistView {
    #[default]
    List,
    Loading { playlist_name: String },
    Tracks  { playlist_name: String, tracks: Vec<Track>, state: ListState },
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    active_tab: usize,

    artists_state:  ListState,
    albums_state:   ListState,
    tracks_state:   ListState,
    playlist_state: ListState,

    artist_view:   ArtistView,
    album_view:    AlbumView,
    playlist_view: PlaylistView,

    server: ServerState,
    modal:  Option<LoginModal>,
    search: Option<SearchState>,
    help_open: bool,

    notification: Option<(String, bool, Instant)>,
    config:       Config,
    cmd_tx:       mpsc::Sender<UiCommand>,
    pb_tx:        mpsc::Sender<PlaybackCommand>,

    playback:       PlaybackState,
    is_loading:     bool,
    queue:          QueueState,
    current_lyrics: Option<LyricsData>,

    // Playlists (lazy-loaded on first visit to tab 4)
    playlists:         Vec<Playlist>,
    playlists_loaded:  bool,
    playlists_loading: bool,

    // Home data (lazy-loaded on first visit to tab 0)
    home_recently_added:  Vec<Album>,
    home_recently_played: Vec<Track>,
    home_loaded:          bool,

    // Settings tab
    settings_selected: usize,
}

impl App {
    fn new(
        config:  Config,
        cmd_tx:  mpsc::Sender<UiCommand>,
        pb_tx:   mpsc::Sender<PlaybackCommand>,
    ) -> Self {
        let has_server   = config.active_server.is_some() && !config.servers.is_empty();
        let server_name  = config.active_server.clone();

        let mut playback = PlaybackState::default();
        playback.volume = config.playback.default_volume;

        let active_tab = tab_id_to_index(&config.ui.startup_tab);

        let mut app = Self {
            active_tab,
            artists_state:  ListState::default(),
            albums_state:   ListState::default(),
            tracks_state:   ListState::default(),
            playlist_state: ListState::default(),
            artist_view:   ArtistView::default(),
            album_view:    AlbumView::default(),
            playlist_view: PlaylistView::default(),
            server:  ServerState::default(),
            modal:   None,
            search:  None,
            help_open: false,
            notification: None,
            config,
            cmd_tx,
            pb_tx,
            playback,
            is_loading:     false,
            queue:          QueueState::default(),
            current_lyrics: None,
            playlists:         Vec::new(),
            playlists_loaded:  false,
            playlists_loading: false,
            home_recently_added:  Vec::new(),
            home_recently_played: Vec::new(),
            home_loaded: false,
            settings_selected: 0,
        };

        if has_server {
            app.server.server_name = server_name;
        } else {
            app.modal = Some(LoginModal::default());
        }

        app
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn server_auth(&self) -> Option<(String, String, String)> {
        let name = self.config.active_server.as_ref()?;
        let srv  = self.config.servers.iter().find(|s| &s.name == name)?;
        let (token, user_id) = secrets::try_get_credentials(&srv.base_url).ok()??;
        Some((srv.base_url.clone(), token, user_id))
    }

    fn stream_url(&self, track_id: &TrackId) -> Option<String> {
        let (base_url, tok, uid) = self.server_auth()?;
        Some(format!(
            "{}/Audio/{}/universal\
             ?UserId={}&api_key={}\
             &MaxStreamingBitrate=140000000\
             &Container=flac,mp3,aac,m4a,ogg,wav,opus",
            base_url, track_id.0, uid, tok,
        ))
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    async fn play_track(&mut self, track: Track) {
        if let Some(prev_id) = self.playback.current.clone() {
            if let Some((base_url, token, user_id)) = self.server_auth() {
                let _ = self.cmd_tx.send(UiCommand::ReportPlaybackStop {
                    track_id: prev_id,
                    position_secs: self.playback.position_secs,
                    base_url, token, user_id,
                }).await;
            }
        }

        let Some(url) = self.stream_url(&track.id) else { return };

        self.playback.current       = Some(track.id.clone());
        self.playback.is_playing    = false;
        self.playback.position_secs = 0.0;
        self.playback.duration_secs = track.duration_secs.map(|s| s as f64).unwrap_or(0.0);
        self.current_lyrics         = None;
        self.is_loading             = true;


        let _ = self.pb_tx.send(PlaybackCommand::Play {
            track_id:   track.id.clone(),
            stream_url: url,
        }).await;

        if let Some((base_url, token, user_id)) = self.server_auth() {
            let _ = self.cmd_tx.send(UiCommand::ReportPlaybackStart {
                track_id: track.id.clone(),
                base_url: base_url.clone(), token: token.clone(), user_id: user_id.clone(),
            }).await;
            let _ = self.cmd_tx.send(UiCommand::FetchLyrics {
                track_id: track.id, base_url, token, user_id,
            }).await;
        }
    }

    async fn advance_queue(&mut self) {
        let repeat = self.playback.repeat;
        if let Some(track) = self.queue.advance(repeat) {
            self.play_track(track).await;
        } else {
            self.playback.current    = None;
            self.playback.is_playing = false;
        }
    }

    // ── Periodic tick ─────────────────────────────────────────────────────────

    fn tick(&mut self) {
        if let Some((_, _, ts)) = &self.notification {
            if ts.elapsed() >= Duration::from_secs(4) {
                self.notification = None;
            }
        }
    }

    // ── List navigation helpers ───────────────────────────────────────────────

    fn current_list_len(&self) -> usize {
        match self.active_tab {
            1 => match &self.artist_view {
                ArtistView::List => self.server.artists.len(),
                ArtistView::Albums { albums, .. } => albums.len(),
                ArtistView::Tracks { tracks, .. } => tracks.len(),
            },
            2 => match &self.album_view {
                AlbumView::List   => self.server.albums.len(),
                AlbumView::Tracks { tracks, .. } => tracks.len(),
            },
            3 => if self.server.tracks.is_empty() { 0 } else { self.server.tracks.len() + 1 },
            4 => match &self.playlist_view {
                PlaylistView::List    => self.playlists.len(),
                PlaylistView::Loading { .. } => 0,
                PlaylistView::Tracks  { tracks, .. } => tracks.len(),
            },
            6 => SETTING_COUNT,
            _ => 0,
        }
    }

    fn current_selected(&self) -> usize {
        match self.active_tab {
            1 => match &self.artist_view {
                ArtistView::List => self.artists_state.selected().unwrap_or(0),
                ArtistView::Albums { state, .. } => state.selected().unwrap_or(0),
                ArtistView::Tracks { state, .. } => state.selected().unwrap_or(0),
            },
            2 => match &self.album_view {
                AlbumView::List   => self.albums_state.selected().unwrap_or(0),
                AlbumView::Tracks { state, .. } => state.selected().unwrap_or(0),
            },
            3 => self.tracks_state.selected().unwrap_or(0),
            4 => match &self.playlist_view {
                PlaylistView::List    => self.playlist_state.selected().unwrap_or(0),
                PlaylistView::Loading { .. } => 0,
                PlaylistView::Tracks  { state, .. } => state.selected().unwrap_or(0),
            },
            6 => self.settings_selected,
            _ => 0,
        }
    }

    fn set_selected(&mut self, next: usize) {
        match self.active_tab {
            1 => {
                match &mut self.artist_view {
                    ArtistView::Albums { state, .. } => { state.select(Some(next)); }
                    ArtistView::Tracks { state, .. } => { state.select(Some(next)); }
                    ArtistView::List => { self.artists_state.select(Some(next)); }
                }
            }
            2 => {
                if let AlbumView::Tracks { state, .. } = &mut self.album_view {
                    state.select(Some(next));
                } else {
                    self.albums_state.select(Some(next));
                }
            }
            3 => self.tracks_state.select(Some(next)),
            4 => {
                if let PlaylistView::Tracks { state, .. } = &mut self.playlist_view {
                    state.select(Some(next));
                } else {
                    self.playlist_state.select(Some(next));
                }
            }
            6 => { self.settings_selected = next; }
            _ => {}
        }
    }

    // ── Tab lazy-loading ──────────────────────────────────────────────────────

    async fn trigger_tab_load(&mut self) {
        match self.active_tab {
            0 if !self.home_loaded => {
                self.home_loaded = true;
                if let Some((base_url, token, user_id)) = self.server_auth() {
                    let _ = self.cmd_tx.send(UiCommand::FetchHomeData { base_url, token, user_id }).await;
                }
            }
            4 if !self.playlists_loaded => {
                self.playlists_loaded  = true;
                self.playlists_loading = true;
                if let Some((base_url, token, user_id)) = self.server_auth() {
                    let _ = self.cmd_tx.send(UiCommand::LoadPlaylists { base_url, token, user_id }).await;
                }
            }
            _ => {}
        }
    }

    fn apply_setting(&mut self, idx: usize) {
        match idx {
            0 => {
                // Cycle startup tab
                self.config.ui.startup_tab = match self.config.ui.startup_tab {
                    TabId::Home      => TabId::Artists,
                    TabId::Artists   => TabId::Albums,
                    TabId::Albums    => TabId::Songs,
                    TabId::Songs     => TabId::Playlists,
                    TabId::Playlists => TabId::Queue,
                    TabId::Queue     => TabId::Settings,
                    TabId::Settings  => TabId::Home,
                };
            }
            _ => return,
        }
        let _ = cfg_loader::save(&self.config);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const SETTING_COUNT: usize = 2;

fn tab_id_to_index(tab: &TabId) -> usize {
    match tab {
        TabId::Home      => 0,
        TabId::Artists   => 1,
        TabId::Albums    => 2,
        TabId::Songs     => 3,
        TabId::Playlists => 4,
        TabId::Queue     => 5,
        TabId::Settings  => 6,
    }
}
