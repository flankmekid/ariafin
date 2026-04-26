use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Tabs},
    Frame, Terminal,
};
use std::{io, time::{Duration, Instant}};
use tokio::sync::mpsc;

use af_core::{
    config::{loader as cfg_loader, Config},
    config::schema::{ServerConfig, TabId},
    events::{AudioEvent, BgEvent, PlaybackCommand, UiCommand},
    types::{
        Album, Artist, AuthToken, LyricsData, Playlist,
        PlaybackState, RepeatMode, Track, TrackId,
    },
};
use af_api::{JellyfinClient, MusicServer};

use crate::input::Action;
use crate::state::{LoginModal, ServerState};
use crate::theme::Theme;
use crate::widgets::draw_login_modal;

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
        match repeat {
            RepeatMode::One => self.current_track().cloned(),
            RepeatMode::All => {
                let next = self.current_idx.map(|i| i + 1).unwrap_or(0);
                self.current_idx = Some(if next >= self.tracks.len() { 0 } else { next });
                self.current_track().cloned()
            }
            RepeatMode::Off => {
                let next = self.current_idx.map(|i| i + 1).unwrap_or(0);
                if next >= self.tracks.len() {
                    self.current_idx = None;
                    None
                } else {
                    self.current_idx = Some(next);
                    self.current_track().cloned()
                }
            }
        }
    }

    fn go_prev(&mut self) -> Option<Track> {
        if let Some(i) = self.current_idx {
            self.current_idx = Some(i.saturating_sub(1));
        }
        self.current_track().cloned()
    }
}

// ── Search overlay state ──────────────────────────────────────────────────────

#[derive(Default)]
struct SearchState {
    query:        String,
    is_searching: bool,
    artists:      Vec<Artist>,
    albums:       Vec<Album>,
    tracks:       Vec<Track>,
    section:      usize, // 0=Artists 1=Albums 2=Tracks
    selected:     usize,
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
        if len == 0 { self.selected = 0; } else { self.selected = self.selected.min(len - 1); }
    }
}

// ── Drill-down navigation state ──────────────────────────────────────────────

enum ArtistView {
    List,
    Albums { artist_name: String, albums: Vec<Album>, state: ListState },
    Tracks {
        artist_name:    String,
        albums:         Vec<Album>,  // kept for Back navigation
        album_idx:      usize,
        album_title:    String,
        album_artist:   String,
        tracks:         Vec<Track>,
        state:          ListState,
    },
}
impl Default for ArtistView { fn default() -> Self { Self::List } }

enum AlbumView {
    List,
    Tracks { album_title: String, album_artist: String, tracks: Vec<Track>, state: ListState },
}
impl Default for AlbumView { fn default() -> Self { Self::List } }

enum PlaylistView {
    List,
    Loading { playlist_name: String },
    Tracks  { playlist_name: String, tracks: Vec<Track>, state: ListState },
}
impl Default for PlaylistView { fn default() -> Self { Self::List } }

// ── App model ─────────────────────────────────────────────────────────────────

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
        Some((srv.base_url.clone(), srv.token.clone()?, srv.user_id.clone()?))
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

    // ── Input handling ────────────────────────────────────────────────────────

    async fn handle(&mut self, action: Action) -> bool {
        if self.modal.is_some() {
            return self.handle_modal_key(action).await;
        }
        // Any key closes help; 'q'/Ctrl-C also quits.
        if self.help_open {
            self.help_open = false;
            return matches!(action, Action::Quit | Action::CharInput('q' | 'Q'));
        }
        if self.search.is_some() {
            return self.handle_search_key(action).await;
        }

        match action {
            // Ctrl-C
            Action::Quit => return true,

            // All printable characters — vim-style command dispatch
            Action::CharInput(c) => match c {
                'q' | 'Q' => return true,

                '1'..='7' => {
                    let i = (c as u8 - b'1') as usize;
                    if i < TABS.len() {
                        self.active_tab = i;
                        self.reset_list_for_tab();
                        self.trigger_tab_load().await;
                    }
                }

                'j' => self.list_select(1),
                'k' => self.list_select(-1),

                ' ' => {
                    if self.playback.is_playing {
                        let _ = self.pb_tx.send(PlaybackCommand::Pause).await;
                    } else if self.playback.current.is_some() {
                        let _ = self.pb_tx.send(PlaybackCommand::Resume).await;
                    }
                }
                'n' => {
                    let _ = self.pb_tx.send(PlaybackCommand::Stop).await;
                    self.advance_queue().await;
                }
                'p' => {
                    let _ = self.pb_tx.send(PlaybackCommand::Stop).await;
                    if let Some(track) = self.queue.go_prev() {
                        self.play_track(track).await;
                    }
                }
                '+' | '=' => {
                    self.playback.volume = self.playback.volume.saturating_add(5).min(100);
                    let v = self.playback.volume;
                    self.config.playback.default_volume = v;
                    let _ = cfg_loader::save(&self.config);
                    let _ = self.pb_tx.send(PlaybackCommand::SetVolume(v)).await;
                }
                '-' => {
                    self.playback.volume = self.playback.volume.saturating_sub(5);
                    let v = self.playback.volume;
                    self.config.playback.default_volume = v;
                    let _ = cfg_loader::save(&self.config);
                    let _ = self.pb_tx.send(PlaybackCommand::SetVolume(v)).await;
                }
                'l' => {
                    let pos = Duration::from_secs_f64(
                        (self.playback.position_secs + 10.0).min(self.playback.duration_secs)
                    );
                    let _ = self.pb_tx.send(PlaybackCommand::Seek(pos)).await;
                }
                'h' => {
                    let pos = Duration::from_secs_f64(
                        (self.playback.position_secs - 10.0).max(0.0)
                    );
                    let _ = self.pb_tx.send(PlaybackCommand::Seek(pos)).await;
                }
                's' => { self.playback.shuffle = !self.playback.shuffle; }
                'r' => { self.playback.repeat = self.playback.repeat.cycle(); }
                'a' | 'A' => { self.modal = Some(LoginModal::default()); }
                '/' => { self.search = Some(SearchState::default()); }
                '?' => { self.help_open = true; }
                _ => {}
            }

            // Esc — navigate back up drill-down stack
            Action::Back => {
                match self.active_tab {
                    1 if matches!(self.artist_view, ArtistView::Tracks { .. }) => {
                        let av = std::mem::take(&mut self.artist_view);
                        if let ArtistView::Tracks { artist_name, albums, album_idx, .. } = av {
                            let mut state = ListState::default();
                            if !albums.is_empty() { state.select(Some(album_idx)); }
                            self.artist_view = ArtistView::Albums { artist_name, albums, state };
                        }
                    }
                    1 if matches!(self.artist_view, ArtistView::Albums { .. }) => {
                        self.artist_view = ArtistView::List;
                    }
                    2 if matches!(self.album_view, AlbumView::Tracks { .. }) => {
                        self.album_view = AlbumView::List;
                    }
                    4 if !matches!(self.playlist_view, PlaylistView::List) => {
                        self.playlist_view = PlaylistView::List;
                    }
                    _ => {}
                }
            }

            Action::TabNext => {
                self.active_tab = (self.active_tab + 1) % TABS.len();
                self.reset_list_for_tab();
                self.trigger_tab_load().await;
            }
            Action::TabPrev => {
                self.active_tab = if self.active_tab == 0 { TABS.len() - 1 } else { self.active_tab - 1 };
                self.reset_list_for_tab();
                self.trigger_tab_load().await;
            }

            Action::ScrollDown     => self.list_select(1),
            Action::ScrollUp       => self.list_select(-1),
            Action::ScrollPageDown => self.list_select(10),
            Action::ScrollPageUp   => self.list_select(-10),

            Action::Enter => self.handle_enter().await,

            // Arrow-key seek (l/h handled above via CharInput)
            Action::SeekForward => {
                let pos = Duration::from_secs_f64(
                    (self.playback.position_secs + 10.0).min(self.playback.duration_secs)
                );
                let _ = self.pb_tx.send(PlaybackCommand::Seek(pos)).await;
            }
            Action::SeekBackward => {
                let pos = Duration::from_secs_f64(
                    (self.playback.position_secs - 10.0).max(0.0)
                );
                let _ = self.pb_tx.send(PlaybackCommand::Seek(pos)).await;
            }

            _ => {}
        }
        false
    }

    async fn handle_enter(&mut self) {
        match self.active_tab {
            3 => {
                let idx = self.tracks_state.selected().unwrap_or(0);
                if self.server.tracks.is_empty() { return; }
                if idx == 0 {
                    // Shuffle Play: pick random track, enable shuffle
                    let n = self.server.tracks.len();
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos() as usize;
                    let rand_idx = seed % n;
                    self.playback.shuffle   = true;
                    self.queue.tracks       = self.server.tracks.clone();
                    self.queue.current_idx  = Some(rand_idx);
                    let track = self.server.tracks[rand_idx].clone();
                    self.play_track(track).await;
                } else {
                    // idx > 0 maps to tracks[idx - 1]
                    let track_idx = idx - 1;
                    if track_idx >= self.server.tracks.len() { return; }
                    self.queue.tracks      = self.server.tracks.clone();
                    self.queue.current_idx = Some(track_idx);
                    let track = self.server.tracks[track_idx].clone();
                    self.play_track(track).await;
                }
            }

            1 => {
                enum Act1 {
                    DrillAlbums(String, Vec<Album>),
                    DrillTracks { artist_name: String, albums: Vec<Album>, album_idx: usize, album_title: String, album_artist: String, tracks: Vec<Track> },
                    Play(Vec<Track>, usize),
                    Noop,
                }
                let act = match &self.artist_view {
                    ArtistView::List => {
                        let idx = self.artists_state.selected().unwrap_or(0);
                        if idx >= self.server.artists.len() { Act1::Noop } else {
                            let artist = &self.server.artists[idx];
                            let mut albums: Vec<Album> = self.server.albums.iter()
                                .filter(|a| a.artist_id.as_ref() == Some(&artist.id))
                                .cloned().collect();
                            albums.sort_by(|a, b| a.year.cmp(&b.year).then(a.title.cmp(&b.title)));
                            Act1::DrillAlbums(artist.name.clone(), albums)
                        }
                    }
                    ArtistView::Albums { albums, state, artist_name } => {
                        let idx = state.selected().unwrap_or(0);
                        if idx >= albums.len() { Act1::Noop } else {
                            let album = &albums[idx];
                            let mut tracks: Vec<Track> = self.server.tracks.iter()
                                .filter(|t| t.album_id.as_ref().map(|i| &i.0) == Some(&album.id.0))
                                .cloned().collect();
                            tracks.sort_by_key(|t| (t.disc_number.unwrap_or(0), t.track_number.unwrap_or(0)));
                            if tracks.is_empty() { Act1::Noop } else {
                                Act1::DrillTracks {
                                    artist_name:  artist_name.clone(),
                                    albums:       albums.clone(),
                                    album_idx:    idx,
                                    album_title:  album.title.clone(),
                                    album_artist: album.artist_name.as_deref().unwrap_or("").to_string(),
                                    tracks,
                                }
                            }
                        }
                    }
                    ArtistView::Tracks { tracks, state, .. } => {
                        let idx = state.selected().unwrap_or(0);
                        if idx >= tracks.len() { Act1::Noop }
                        else { Act1::Play(tracks.clone(), idx) }
                    }
                };
                match act {
                    Act1::DrillAlbums(name, albums) => {
                        let mut state = ListState::default();
                        if !albums.is_empty() { state.select(Some(0)); }
                        self.artist_view = ArtistView::Albums { artist_name: name, albums, state };
                    }
                    Act1::DrillTracks { artist_name, albums, album_idx, album_title, album_artist, tracks } => {
                        let mut state = ListState::default();
                        if !tracks.is_empty() { state.select(Some(0)); }
                        self.artist_view = ArtistView::Tracks {
                            artist_name, albums, album_idx,
                            album_title, album_artist, tracks, state,
                        };
                    }
                    Act1::Play(tracks, idx) => {
                        let track = tracks[idx].clone();
                        self.queue.tracks      = tracks;
                        self.queue.current_idx = Some(idx);
                        self.play_track(track).await;
                    }
                    Act1::Noop => {}
                }
            }

            2 => {
                enum Act2 { Drill(String, String, Vec<Track>), Play(Vec<Track>, usize), Noop }
                let act = match &self.album_view {
                    AlbumView::List => {
                        let idx = self.albums_state.selected().unwrap_or(0);
                        if idx >= self.server.albums.len() { Act2::Noop } else {
                            let album = &self.server.albums[idx];
                            let mut tracks: Vec<Track> = self.server.tracks.iter()
                                .filter(|t| t.album_id.as_ref() == Some(&album.id))
                                .cloned().collect();
                            tracks.sort_by_key(|t| t.track_number.unwrap_or(0));
                            if tracks.is_empty() { Act2::Noop } else {
                                Act2::Drill(
                                    album.title.clone(),
                                    album.artist_name.as_deref().unwrap_or("").to_string(),
                                    tracks,
                                )
                            }
                        }
                    }
                    AlbumView::Tracks { tracks, state, .. } => {
                        let idx = state.selected().unwrap_or(0);
                        if idx >= tracks.len() { Act2::Noop }
                        else { Act2::Play(tracks.clone(), idx) }
                    }
                };
                match act {
                    Act2::Drill(title, artist, tracks) => {
                        let mut state = ListState::default();
                        if !tracks.is_empty() { state.select(Some(0)); }
                        self.album_view = AlbumView::Tracks {
                            album_title: title, album_artist: artist, tracks, state,
                        };
                    }
                    Act2::Play(tracks, idx) => {
                        let track = tracks[idx].clone();
                        self.queue.tracks      = tracks;
                        self.queue.current_idx = Some(idx);
                        self.play_track(track).await;
                    }
                    Act2::Noop => {}
                }
            }

            6 => {
                self.apply_setting(self.settings_selected);
            }

            4 => {
                if matches!(&self.playlist_view, PlaylistView::Tracks { .. }) {
                    // Play the selected track
                    enum Act4 { Play(Vec<Track>, usize), Noop }
                    let act = match &self.playlist_view {
                        PlaylistView::Tracks { tracks, state, .. } => {
                            let idx = state.selected().unwrap_or(0);
                            if idx < tracks.len() { Act4::Play(tracks.clone(), idx) }
                            else { Act4::Noop }
                        }
                        _ => Act4::Noop,
                    };
                    if let Act4::Play(tracks, idx) = act {
                        let track = tracks[idx].clone();
                        self.queue.tracks      = tracks;
                        self.queue.current_idx = Some(idx);
                        self.play_track(track).await;
                    }
                } else if matches!(&self.playlist_view, PlaylistView::List) {
                    // Drill into the playlist to show its tracks
                    let idx = self.playlist_state.selected().unwrap_or(0);
                    if idx >= self.playlists.len() { return; }
                    let pid           = self.playlists[idx].id.clone();
                    let playlist_name = self.playlists[idx].name.clone();
                    if let Some((base_url, token, user_id)) = self.server_auth() {
                        self.playlist_view = PlaylistView::Loading { playlist_name };
                        let _ = self.cmd_tx.send(UiCommand::LoadPlaylistTracks {
                            playlist_id: pid, base_url, token, user_id,
                        }).await;
                    }
                }
            }
            _ => {}
        }
    }

    // ── Search key handling ───────────────────────────────────────────────────

    async fn handle_search_key(&mut self, action: Action) -> bool {
        let search = match &mut self.search { Some(s) => s, None => return false };

        match action {
            Action::Quit => return true,   // Ctrl-C quits even from search
            Action::Back => { self.search = None; }

            // Every printable character types into the query (no vim stealing)
            Action::CharInput(c) => {
                search.query.push(c);
            }
            Action::Backspace => { search.query.pop(); }

            Action::Enter => {
                // If there are results and something is selected, play it.
                if search.has_results() {
                    let section  = search.section;
                    let selected = search.selected;

                    match section {
                        0 => {
                            if let Some(artist) = search.artists.get(selected).cloned() {
                                let mut tracks: Vec<Track> = self.server.tracks.iter()
                                    .filter(|t| t.artist_id.as_ref().map(|id| &id.0) == Some(&artist.id.0))
                                    .cloned().collect();
                                tracks.sort_by_key(|t| t.title.clone());
                                if !tracks.is_empty() {
                                    let first = tracks[0].clone();
                                    self.queue.tracks      = tracks;
                                    self.queue.current_idx = Some(0);
                                    self.search = None;
                                    self.play_track(first).await;
                                }
                            }
                        }
                        1 => {
                            if let Some(album) = search.albums.get(selected).cloned() {
                                let mut tracks: Vec<Track> = self.server.tracks.iter()
                                    .filter(|t| t.album_id.as_ref().map(|id| &id.0) == Some(&album.id.0))
                                    .cloned().collect();
                                tracks.sort_by_key(|t| t.track_number.unwrap_or(0));
                                if !tracks.is_empty() {
                                    let first = tracks[0].clone();
                                    self.queue.tracks      = tracks;
                                    self.queue.current_idx = Some(0);
                                    self.search = None;
                                    self.play_track(first).await;
                                }
                            }
                        }
                        _ => {
                            if let Some(track) = search.tracks.get(selected).cloned() {
                                // Queue the full track results from the search
                                let all_tracks = search.tracks.clone();
                                self.queue.tracks      = all_tracks;
                                self.queue.current_idx = Some(selected);
                                self.search = None;
                                self.play_track(track).await;
                            }
                        }
                    }
                } else if !search.query.is_empty() {
                    // Clone query before server_auth() needs &self (NLL ends search borrow here)
                    let query = search.query.clone();
                    if let Some((base_url, token, user_id)) = self.server_auth() {
                        if let Some(s) = &mut self.search { s.is_searching = true; }
                        let _ = self.cmd_tx.send(UiCommand::Search {
                            query, base_url, token, user_id,
                        }).await;
                    }
                }
            }

            Action::TabNext => {
                let s = self.search.as_mut().unwrap();
                s.section  = (s.section + 1) % 3;
                s.selected = 0;
                s.clamp_selected();
            }
            Action::TabPrev => {
                let s = self.search.as_mut().unwrap();
                s.section  = if s.section == 0 { 2 } else { s.section - 1 };
                s.selected = 0;
                s.clamp_selected();
            }

            Action::ScrollDown => {
                let s   = self.search.as_mut().unwrap();
                let len = s.section_len();
                if len > 0 { s.selected = (s.selected + 1).min(len - 1); }
            }
            Action::ScrollUp => {
                let s = self.search.as_mut().unwrap();
                s.selected = s.selected.saturating_sub(1);
            }

            _ => {}
        }
        false
    }

    // ── Login modal ───────────────────────────────────────────────────────────

    async fn handle_modal_key(&mut self, action: Action) -> bool {
        let modal = match &mut self.modal { Some(m) => m, None => return false };
        if modal.submitting { return false; }

        match action {
            // Ctrl-C quits even from the login form
            Action::Quit => return true,
            // Esc closes the modal (only if a server is already configured)
            Action::Back => {
                if self.config.active_server.is_some() { self.modal = None; }
            }

            // Tab / arrow keys cycle between URL, username, password fields
            Action::TabNext | Action::ScrollDown => { let f = modal.focused.next(); modal.focused = f; }
            Action::TabPrev | Action::ScrollUp   => { let f = modal.focused.prev(); modal.focused = f; }

            Action::Enter => {
                let url      = modal.url.trim().to_string();
                let username = modal.username.trim().to_string();
                let password = modal.password.clone();
                if url.is_empty() || username.is_empty() || password.is_empty() {
                    if let Some(m) = &mut self.modal { m.error = Some("All fields are required".into()); }
                    return false;
                }
                let server_name = format!("{} ({})", username, url);
                if let Some(m) = &mut self.modal { m.submitting = true; m.error = None; }
                let _ = self.cmd_tx.send(UiCommand::Authenticate {
                    server_name, base_url: url, username, password,
                }).await;
            }

            // Every printable character (including q j k h l n p s r / ?) types into the field
            Action::CharInput(c) => { modal.focused_field_mut().push(c); modal.error = None; }
            Action::Backspace    => { modal.focused_field_mut().pop(); }
            _ => {}
        }
        false
    }

    fn reset_list_for_tab(&mut self) {
        // Always reset drill-down state when switching tabs
        self.artist_view   = ArtistView::List;
        self.album_view    = AlbumView::List;
        self.playlist_view = PlaylistView::List;
        match self.active_tab {
            1 => { if !self.server.artists.is_empty() { self.artists_state.select(Some(0)); } }
            2 => { if !self.server.albums.is_empty()  { self.albums_state.select(Some(0)); } }
            3 => { if !self.server.tracks.is_empty()  { self.tracks_state.select(Some(0)); } }
            4 => { if !self.playlists.is_empty()      { self.playlist_state.select(Some(0)); } }
            _ => {}
        }
    }

    fn list_select(&mut self, delta: i32) {
        let len = self.current_list_len();
        if len == 0 { return; }
        let cur  = self.current_selected() as i32;
        let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
        self.set_selected(next);
    }

    // ── Background event handling ─────────────────────────────────────────────

    fn handle_bg_event(&mut self, event: BgEvent) {
        match event {
            BgEvent::AuthSuccess { server_name, token, user_id } => {
                // Extract base_url and username before any mutation (borrow-safe).
                let (base_url, modal_username) = {
                    let existing = self.config.servers.iter().find(|s| s.name == server_name);
                    if let Some(s) = existing {
                        (s.base_url.clone(), s.username.clone())
                    } else if let Some(m) = &self.modal {
                        (m.url.clone(), m.username.clone())
                    } else {
                        return; // impossible state
                    }
                };

                if let Some(s) = self.config.servers.iter_mut().find(|s| s.name == server_name) {
                    s.token   = Some(token.clone());
                    s.user_id = Some(user_id.clone());
                } else {
                    self.config.servers.push(ServerConfig {
                        name: server_name.clone(),
                        server_type: af_core::config::schema::ServerType::Jellyfin,
                        base_url: base_url.clone(),
                        username: modal_username,
                        token:    Some(token.clone()),
                        user_id:  Some(user_id.clone()),
                    });
                    self.config.active_server = Some(server_name.clone());
                }
                let _ = cfg_loader::save(&self.config);
                self.modal = None;
                self.notification = Some(("Connected! Syncing library…".into(), false, Instant::now()));
                self.server.server_name = Some(server_name.clone());
                let tx  = self.cmd_tx.clone();
                let sn  = server_name.clone();
                let url = base_url.clone();
                let tok = token.clone();
                let uid = user_id.clone();
                tokio::spawn(async move {
                    let _ = tx.send(UiCommand::StartSync {
                        server_name: sn,
                        base_url:    url,
                        token:       tok,
                        user_id:     uid,
                    }).await;
                });
            }

            BgEvent::AuthFailed(msg) => {
                if let Some(m) = &mut self.modal { m.submitting = false; m.error = Some(msg); }
            }

            BgEvent::SyncStarted => {
                self.server.is_syncing = true;
                self.server.sync_label = "Syncing…".into();
                self.server.sync_done  = 0;
                self.server.sync_total = 0;
            }
            BgEvent::SyncProgress { label, done, total } => {
                self.server.sync_label = label;
                self.server.sync_done  = done;
                self.server.sync_total = total;
            }
            BgEvent::SyncComplete => {
                self.server.is_syncing = false;
                self.notification = Some(("Library synced.".into(), false, Instant::now()));
            }
            BgEvent::SyncFailed(e) => {
                self.server.is_syncing = false;
                self.notification = Some((format!("Sync failed: {e}"), true, Instant::now()));
            }

            BgEvent::ArtistsLoaded(artists) => {
                self.server.artists = artists;
                if !self.server.artists.is_empty() && self.active_tab == 1 {
                    self.artists_state.select(Some(0));
                }
            }
            BgEvent::AlbumsLoaded(albums) => {
                self.server.albums = albums;
                if !self.server.albums.is_empty() && self.active_tab == 2 {
                    self.albums_state.select(Some(0));
                }
            }
            BgEvent::TracksLoaded(tracks) => {
                self.server.tracks = tracks;
                if !self.server.tracks.is_empty() && self.active_tab == 3 {
                    self.tracks_state.select(Some(0));
                }
            }

            BgEvent::LyricsLoaded(lyrics) => {
                self.current_lyrics = lyrics;
            }

            BgEvent::PlaylistsLoaded(playlists) => {
                self.playlists         = playlists;
                self.playlists_loading = false;
                if !self.playlists.is_empty() && self.active_tab == 4 {
                    self.playlist_state.select(Some(0));
                }
            }

            BgEvent::PlaylistTracksReady(tracks) => {
                let playlist_name = match &self.playlist_view {
                    PlaylistView::Loading { playlist_name } => playlist_name.clone(),
                    _ => String::new(),
                };
                let mut state = ListState::default();
                if !tracks.is_empty() { state.select(Some(0)); }
                self.playlist_view = PlaylistView::Tracks { playlist_name, tracks, state };
            }

            BgEvent::HomeDataLoaded { recently_added, recently_played } => {
                self.home_recently_added  = recently_added;
                self.home_recently_played = recently_played;
            }

            BgEvent::SearchLoaded { artists, albums, tracks } => {
                if let Some(s) = &mut self.search {
                    s.is_searching = false;
                    s.artists  = artists;
                    s.albums   = albums;
                    s.tracks   = tracks;
                    s.section  = if !s.tracks.is_empty() { 2 } else if !s.albums.is_empty() { 1 } else { 0 };
                    s.selected = 0;
                }
            }
        }
    }

    // ── Audio event handling ──────────────────────────────────────────────────

    fn handle_audio_event(&mut self, event: AudioEvent) {
        match event {
            AudioEvent::StateChanged { is_playing } => {
                self.playback.is_playing = is_playing;
                if is_playing { self.is_loading = false; }
            }
            AudioEvent::PositionChanged { position, duration } => {
                self.playback.position_secs = position.as_secs_f64();
                // Prefer metadata duration (set at play time); only fall back to the
                // audio engine's value when we have nothing from the cache.
                if self.playback.duration_secs == 0.0 {
                    self.playback.duration_secs = duration.as_secs_f64();
                }
            }
            AudioEvent::TrackChanged(Some(id)) => {
                self.playback.current = Some(id);
            }
            AudioEvent::TrackChanged(None) => {
                if let Some(prev_id) = self.playback.current.clone() {
                    if let Some((base_url, token, user_id)) = self.server_auth() {
                        let pos = self.playback.position_secs;
                        let tx  = self.cmd_tx.clone();
                        tokio::spawn(async move {
                            let _ = tx.send(UiCommand::ReportPlaybackStop {
                                track_id: prev_id, position_secs: pos, base_url, token, user_id,
                            }).await;
                        });
                    }
                }
                self.current_lyrics = None;

                let repeat = self.playback.repeat;
                if let Some(next) = self.queue.advance(repeat) {
                    self.playback.current       = Some(next.id.clone());
                    self.playback.is_playing    = false;
                    self.playback.position_secs = 0.0;
                    self.playback.duration_secs = next.duration_secs.map(|s| s as f64).unwrap_or(0.0);
                    self.is_loading             = true;
                    let pb_tx  = self.pb_tx.clone();
                    let cmd_tx = self.cmd_tx.clone();
                    let auth   = self.server_auth();
                    let url    = self.stream_url(&next.id);
                    tokio::spawn(async move {
                        if let Some(url) = url {
                            let _ = pb_tx.send(PlaybackCommand::Play {
                                track_id: next.id.clone(), stream_url: url,
                            }).await;
                        }
                        if let Some((base_url, token, user_id)) = auth {
                            let _ = cmd_tx.send(UiCommand::ReportPlaybackStart {
                                track_id: next.id.clone(),
                                base_url: base_url.clone(), token: token.clone(), user_id: user_id.clone(),
                            }).await;
                            let _ = cmd_tx.send(UiCommand::FetchLyrics {
                                track_id: next.id, base_url, token, user_id,
                            }).await;
                        }
                    });
                } else {
                    self.playback.current    = None;
                    self.playback.is_playing = false;
                }
            }
            AudioEvent::Error(e) => {
                tracing::error!("audio: {e}");
                self.notification = Some((format!("Audio error: {e}"), true, Instant::now()));
                self.playback.is_playing = false;
                self.is_loading = false;
            }
        }
    }
}

// ── Main run loop ─────────────────────────────────────────────────────────────

pub async fn run(config: Config) -> Result<()> {
    let (cmd_tx, cmd_rx)   = mpsc::channel::<UiCommand>(32);
    let (bg_tx, mut bg_rx) = mpsc::channel::<BgEvent>(64);
    let (pb_tx, pb_rx)     = mpsc::channel::<PlaybackCommand>(32);
    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioEvent>(64);

    tokio::spawn(background_worker(cmd_rx, bg_tx));
    af_audio::spawn(pb_rx, audio_tx);

    if let Some(server_name) = config.active_server.clone() {
        if let Some(srv) = config.servers.iter().find(|s| s.name == server_name) {
            if let (Some(token), Some(user_id)) = (srv.token.clone(), srv.user_id.clone()) {
                let base_url = srv.base_url.clone();

                // Show any previously cached data immediately while the sync runs.
                let _ = cmd_tx.send(UiCommand::LoadFromCache {
                    server_name: server_name.clone(),
                }).await;

                // Trigger an immediate sync so fresh data appears without waiting 30 min.
                let _ = cmd_tx.send(UiCommand::StartSync {
                    server_name: server_name.clone(),
                    base_url:    base_url.clone(),
                    token:       token.clone(),
                    user_id:     user_id.clone(),
                }).await;

                // Periodic re-sync every 30 minutes after that.
                let sync_tx = cmd_tx.clone();
                tokio::spawn(af_daemon::run_sync_service(
                    server_name,
                    base_url,
                    token,
                    user_id,
                    sync_tx,
                    Duration::from_secs(1800),
                ));
            }
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = run_loop(&mut terminal, config, cmd_tx, &mut bg_rx, pb_tx, &mut audio_rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn run_loop(
    terminal:  &mut Terminal<CrosstermBackend<io::Stdout>>,
    config:    Config,
    cmd_tx:    mpsc::Sender<UiCommand>,
    bg_rx:     &mut mpsc::Receiver<BgEvent>,
    pb_tx:     mpsc::Sender<PlaybackCommand>,
    audio_rx:  &mut mpsc::Receiver<AudioEvent>,
) -> Result<()> {
    let mut app    = App::new(config, cmd_tx, pb_tx);
    let mut events = EventStream::new();

    // Apply persisted volume to audio engine and trigger load for startup tab.
    let v = app.playback.volume;
    let _ = app.pb_tx.send(PlaybackCommand::SetVolume(v)).await;
    app.trigger_tab_load().await;

    loop {
        terminal.draw(|frame| draw(frame, &mut app))?;

        tokio::select! {
            biased;

            maybe_ev = events.next() => {
                match maybe_ev {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        let action = map_key(key.code, key.modifiers);
                        if app.handle(action).await { break; }
                    }
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }

            Some(event) = bg_rx.recv()    => { app.handle_bg_event(event);   }
            Some(event) = audio_rx.recv() => { app.handle_audio_event(event); }

            _ = tokio::time::sleep(Duration::from_millis(100)) => { app.tick(); }
        }
    }
    Ok(())
}

// ── Background worker ─────────────────────────────────────────────────────────

async fn background_worker(
    mut cmd_rx: mpsc::Receiver<UiCommand>,
    bg_tx: mpsc::Sender<BgEvent>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        let tx = bg_tx.clone();
        tokio::spawn(async move { handle_command(cmd, tx).await; });
    }
}

async fn handle_command(cmd: UiCommand, tx: mpsc::Sender<BgEvent>) {
    match cmd {
        UiCommand::Authenticate { server_name, base_url, username, password } => {
            let client = JellyfinClient::new(&server_name, &base_url);
            match client.authenticate(&username, &password).await {
                Ok(token) => {
                    let _ = tx.send(BgEvent::AuthSuccess {
                        server_name, token: token.token, user_id: token.user_id,
                    }).await;
                }
                Err(e) => { let _ = tx.send(BgEvent::AuthFailed(e.to_string())).await; }
            }
        }

        UiCommand::StartSync { server_name, base_url, token, user_id } => {
            let auth   = AuthToken { token, user_id };
            let client = JellyfinClient::new(&server_name, &base_url);
            let _ = tx.send(BgEvent::SyncStarted).await;
            perform_sync(client, auth, server_name, tx).await;
        }

        UiCommand::LoadFromCache { server_name } => {
            let sn  = server_name.clone();
            let tx2 = tx.clone();
            match tokio::task::spawn_blocking(move || load_from_cache(&sn)).await {
                Ok(Ok((artists, albums, tracks))) => {
                    let _ = tx2.send(BgEvent::ArtistsLoaded(artists)).await;
                    let _ = tx2.send(BgEvent::AlbumsLoaded(albums)).await;
                    let _ = tx2.send(BgEvent::TracksLoaded(tracks)).await;
                }
                Ok(Err(e)) => tracing::warn!("cache load: {e}"),
                Err(e)     => tracing::warn!("cache task: {e}"),
            }
        }

        UiCommand::ReportPlaybackStart { track_id, base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            if let Err(e) = client.report_playback_start(&auth, &track_id).await {
                tracing::warn!("report start: {e}");
            }
        }

        UiCommand::ReportPlaybackStop { track_id, position_secs, base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            if let Err(e) = client.report_playback_stop(&auth, &track_id, position_secs).await {
                tracing::warn!("report stop: {e}");
            }
        }

        UiCommand::FetchLyrics { track_id, base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            let lyrics = client.get_lyrics(&auth, &track_id).await.unwrap_or(None);
            let _ = tx.send(BgEvent::LyricsLoaded(lyrics)).await;
        }

        UiCommand::LoadPlaylists { base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            match client.get_playlists(&auth).await {
                Ok(playlists) => { let _ = tx.send(BgEvent::PlaylistsLoaded(playlists)).await; }
                Err(e)        => tracing::warn!("load playlists: {e}"),
            }
        }

        UiCommand::LoadPlaylistTracks { playlist_id, base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            match client.get_playlist_tracks(&auth, &playlist_id).await {
                Ok(tracks) => { let _ = tx.send(BgEvent::PlaylistTracksReady(tracks)).await; }
                Err(e)     => tracing::warn!("load playlist tracks: {e}"),
            }
        }

        UiCommand::FetchHomeData { base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            let recently_added  = client.get_recently_added(&auth, 10).await.unwrap_or_default();
            let recently_played = client.get_recently_played(&auth, 10).await.unwrap_or_default();
            let _ = tx.send(BgEvent::HomeDataLoaded { recently_added, recently_played }).await;
        }

        UiCommand::Search { query, base_url, token, user_id } => {
            let client = JellyfinClient::new("", &base_url);
            let auth   = AuthToken { token, user_id };
            match client.search(&auth, &query, 20).await {
                Ok(results) => {
                    let _ = tx.send(BgEvent::SearchLoaded {
                        artists: results.artists,
                        albums:  results.albums,
                        tracks:  results.tracks,
                    }).await;
                }
                Err(e) => {
                    tracing::warn!("search: {e}");
                    let _ = tx.send(BgEvent::SearchLoaded {
                        artists: vec![], albums: vec![], tracks: vec![],
                    }).await;
                }
            }
        }
    }
}

fn load_from_cache(server_name: &str) -> anyhow::Result<(
    Vec<af_core::types::Artist>,
    Vec<af_core::types::Album>,
    Vec<af_core::types::Track>,
)> {
    let db = af_core::cache::CacheDb::open(server_name)?;
    Ok((db.load_artists()?, db.load_albums()?, db.load_tracks()?))
}

async fn perform_sync(
    client: JellyfinClient,
    token: AuthToken,
    server_name: String,
    tx: mpsc::Sender<BgEvent>,
) {
    macro_rules! progress {
        ($label:expr, $done:expr, $total:expr) => {
            let _ = tx.send(BgEvent::SyncProgress {
                label: $label.into(), done: $done, total: $total,
            }).await;
        };
    }

    progress!("Fetching artists…", 0, 0);
    let artists = match client.get_artists(&token).await {
        Ok(a)  => a,
        Err(e) => { let _ = tx.send(BgEvent::SyncFailed(e.to_string())).await; return; }
    };
    let na = artists.len() as u32;

    progress!("Fetching albums…", na, na * 3);
    let albums = match client.get_albums(&token, af_core::types::AlbumFilter::default()).await {
        Ok(a)  => a,
        Err(e) => { let _ = tx.send(BgEvent::SyncFailed(e.to_string())).await; return; }
    };
    let nb = albums.len() as u32;

    progress!("Fetching tracks…", na + nb, (na + nb) * 2);
    let tracks = match client.get_tracks(&token, af_core::types::TrackFilter::default()).await {
        Ok(t)  => t,
        Err(e) => { let _ = tx.send(BgEvent::SyncFailed(e.to_string())).await; return; }
    };

    let sn = server_name.clone();
    let (a2, al2, t2, tx2) = (artists.clone(), albums.clone(), tracks.clone(), tx.clone());
    match tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let db = af_core::cache::CacheDb::open(&sn)?;
        db.upsert_artists(&a2)?;
        db.upsert_albums(&al2)?;
        db.upsert_tracks(&t2)?;
        Ok(())
    }).await {
        Ok(Ok(())) => {
            let _ = tx.send(BgEvent::ArtistsLoaded(artists)).await;
            let _ = tx.send(BgEvent::AlbumsLoaded(albums)).await;
            let _ = tx.send(BgEvent::TracksLoaded(tracks)).await;
            let _ = tx.send(BgEvent::SyncComplete).await;
        }
        Ok(Err(e)) => { let _ = tx2.send(BgEvent::SyncFailed(e.to_string())).await; }
        Err(e)     => { let _ = tx2.send(BgEvent::SyncFailed(e.to_string())).await; }
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

// ── Key mapping ───────────────────────────────────────────────────────────────

fn map_key(code: KeyCode, mods: KeyModifiers) -> Action {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    match code {
        // Ctrl combos
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('d') if ctrl => Action::ScrollPageDown,
        KeyCode::Char('u') if ctrl => Action::ScrollPageUp,

        // Non-char keys
        KeyCode::Tab       => Action::TabNext,
        KeyCode::BackTab   => Action::TabPrev,
        KeyCode::Down      => Action::ScrollDown,
        KeyCode::Up        => Action::ScrollUp,
        KeyCode::Right     => Action::SeekForward,
        KeyCode::Left      => Action::SeekBackward,
        KeyCode::PageDown  => Action::ScrollPageDown,
        KeyCode::PageUp    => Action::ScrollPageUp,
        KeyCode::Enter     => Action::Enter,
        KeyCode::Esc       => Action::Back,
        KeyCode::Backspace => Action::Backspace,

        // All printable chars become CharInput — context handlers dispatch from there.
        // This ensures modal / search text fields receive every character as typed.
        KeyCode::Char(c) if !ctrl => Action::CharInput(c),
        _ => Action::None,
    }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let player_h = if app.playback.current.is_some() { 6 } else { 3 };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(player_h),
        ])
        .split(area);

    draw_tab_bar(frame, outer[0], app);
    draw_content(frame, outer[1], app);
    draw_player_bar(frame, outer[2], app);

    if let Some(modal) = &app.modal {
        draw_login_modal(frame, modal);
    }
    if let Some(search) = &app.search {
        draw_search_overlay(frame, search, area);
    }
    if app.help_open {
        draw_help_overlay(frame, area);
    }
}

fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = TABS.iter().enumerate().map(|(i, &t)| {
        if i == app.active_tab {
            Line::from(Span::styled(t, Theme::tab_active()))
        } else {
            Line::from(Span::styled(t, Theme::tab_inactive()))
        }
    }).collect();

    let tabs = Tabs::new(titles)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::border())
            .title(Span::styled(" ariafin ", Theme::accent_bold())))
        .select(app.active_tab)
        .highlight_style(Theme::tab_active().add_modifier(Modifier::UNDERLINED));

    frame.render_widget(tabs, area);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.active_tab {
        0 => draw_home(frame, area, app),
        1 => draw_artists(frame, area, app),
        2 => draw_albums(frame, area, app),
        3 => draw_songs(frame, area, app),
        4 => draw_playlists(frame, area, app),
        5 => draw_queue(frame, area, app),
        6 => draw_settings(frame, area, app),
        _ => {}
    }
}

// ── Home ──────────────────────────────────────────────────────────────────────

fn draw_home(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(" Home ", Theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let has_home_data = !app.home_recently_added.is_empty() || !app.home_recently_played.is_empty();

    if !has_home_data {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Min(0)])
            .split(inner);

        let summary = if let Some(name) = &app.server.server_name {
            format!(
                "  Connected to {}  ·  {} artists  ·  {} albums  ·  {} tracks",
                name,
                app.server.artists.len(),
                app.server.albums.len(),
                app.server.tracks.len(),
            )
        } else {
            "  No server connected.  Go to Settings to add one.".into()
        };

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("Welcome to ariafin", Theme::accent_bold())),
                Line::from(""),
                Line::from(Span::styled(summary, Theme::secondary())),
                Line::from(""),
                Line::from(Span::styled(
                    "  Visit this tab again to load recently added / recently played.",
                    Theme::muted(),
                )),
            ])
            .alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    // Split: header | recently added | recently played
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Percentage(50), Constraint::Min(0)])
        .split(inner);

    // Header summary
    let summary = if let Some(name) = &app.server.server_name {
        format!(
            "  {}  ·  {} artists  ·  {} albums  ·  {} tracks",
            name,
            app.server.artists.len(),
            app.server.albums.len(),
            app.server.tracks.len(),
        )
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Welcome to ariafin", Theme::accent_bold())),
            Line::from(Span::styled(summary, Theme::secondary())),
        ]),
        sections[0],
    );

    // Recently added albums
    let added_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Theme::border())
        .title(Span::styled(" Recently Added ", Theme::accent()));
    let added_items: Vec<ListItem> = app.home_recently_added.iter().map(|a| {
        let year   = a.year.map(|y| format!("  {y}")).unwrap_or_default();
        let artist = a.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled("  ", Theme::muted()),
            Span::styled(&a.title, Theme::normal()),
            Span::styled(year, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }).collect();
    frame.render_widget(List::new(added_items).block(added_block), sections[1]);

    // Recently played tracks
    let played_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Theme::border())
        .title(Span::styled(" Recently Played ", Theme::accent()));
    let played_items: Vec<ListItem> = app.home_recently_played.iter().map(|t| {
        let dur    = t.duration_secs.map(|s| format!("  {}:{:02}", s/60, s%60)).unwrap_or_default();
        let artist = t.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled("  ", Theme::muted()),
            Span::styled(&t.title, Theme::normal()),
            Span::styled(dur, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }).collect();
    frame.render_widget(List::new(played_items).block(played_block), sections[2]);
}

// ── Artists ───────────────────────────────────────────────────────────────────

fn draw_artists(frame: &mut Frame, area: Rect, app: &mut App) {
    if let ArtistView::Tracks { artist_name, album_title, album_artist, tracks, state, .. } = &mut app.artist_view {
        draw_artist_tracks(frame, area, artist_name, album_title, album_artist, tracks, state, app.playback.current.as_ref());
        return;
    }
    if let ArtistView::Albums { artist_name, albums, state } = &mut app.artist_view {
        draw_artist_albums(frame, area, artist_name, albums, state);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(
            format!(" Artists ({})  — Enter to expand ", app.server.artists.len()),
            Theme::accent(),
        ));

    if app.server.artists.is_empty() {
        let msg = if app.server.is_syncing {
            format!("  {}  {}/{}", app.server.sync_label, app.server.sync_done, app.server.sync_total)
        } else {
            "  No artists. Connect a server via Settings.".into()
        };
        frame.render_widget(Paragraph::new(msg).block(block).style(Theme::secondary()), area);
        return;
    }

    let items: Vec<ListItem> = app.server.artists.iter().map(|a| {
        let count = if a.album_count > 0 { format!("  {} albums", a.album_count) } else { String::new() };
        ListItem::new(Line::from(vec![
            Span::styled(&a.name, Theme::normal()),
            Span::styled(count, Theme::muted()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, &mut app.artists_state,
    );
}

fn draw_artist_albums(frame: &mut Frame, area: Rect, artist_name: &str, albums: &[Album], state: &mut ListState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(
            format!(" Artists > {}  — Enter play · Esc back ", artist_name),
            Theme::accent_bold(),
        ));

    if albums.is_empty() {
        frame.render_widget(
            Paragraph::new("  No albums for this artist.").block(block).style(Theme::secondary()),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = albums.iter().map(|a| {
        let year   = a.year.map(|y| format!("  {y}")).unwrap_or_default();
        let artist = a.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled(&a.title, Theme::normal()),
            Span::styled(year, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, state,
    );
}

fn draw_artist_tracks(
    frame: &mut Frame,
    area: Rect,
    artist_name: &str,
    album_title: &str,
    album_artist: &str,
    tracks: &[Track],
    state: &mut ListState,
    current_id: Option<&TrackId>,
) {
    let artist_part = if album_artist.is_empty() {
        String::new()
    } else {
        format!("  by {album_artist}")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(
            format!(" Artists > {artist_name} > {album_title}{artist_part}  — Enter play · Esc back "),
            Theme::accent_bold(),
        ));

    if tracks.is_empty() {
        frame.render_widget(
            Paragraph::new("  No tracks in this album.").block(block).style(Theme::secondary()),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = tracks.iter().map(|t| {
        let playing = current_id == Some(&t.id);
        let num  = t.track_number.map(|n| format!("{n:>2}. ")).unwrap_or_else(|| "    ".into());
        let dur  = t.duration_secs.map(|s| format!("  {}:{:02}", s/60, s%60)).unwrap_or_default();
        ListItem::new(Line::from(vec![
            Span::styled(num, Theme::muted()),
            Span::styled(if playing { "♪ " } else { "  " }, Theme::accent()),
            Span::styled(&t.title, if playing { Theme::accent_bold() } else { Theme::normal() }),
            Span::styled(dur, Theme::muted()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, state,
    );
}

// ── Albums ────────────────────────────────────────────────────────────────────

fn draw_albums(frame: &mut Frame, area: Rect, app: &mut App) {
    if let AlbumView::Tracks { album_title, album_artist, tracks, state } = &mut app.album_view {
        draw_album_tracks(frame, area, album_title, album_artist, tracks, state, app.playback.current.as_ref());
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(
            format!(" Albums ({})  — Enter to expand ", app.server.albums.len()),
            Theme::accent(),
        ));

    if app.server.albums.is_empty() {
        frame.render_widget(Paragraph::new("  No albums.").block(block).style(Theme::secondary()), area);
        return;
    }

    let items: Vec<ListItem> = app.server.albums.iter().map(|a| {
        let year   = a.year.map(|y| format!("  {y}")).unwrap_or_default();
        let artist = a.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled(&a.title, Theme::normal()),
            Span::styled(year, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, &mut app.albums_state,
    );
}

fn draw_album_tracks(
    frame: &mut Frame,
    area: Rect,
    album_title: &str,
    album_artist: &str,
    tracks: &[Track],
    state: &mut ListState,
    current_id: Option<&TrackId>,
) {
    let artist_part = if album_artist.is_empty() {
        String::new()
    } else {
        format!("  by {album_artist}")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(
            format!(" Albums > {album_title}{artist_part}  — Enter play · Esc back "),
            Theme::accent_bold(),
        ));

    if tracks.is_empty() {
        frame.render_widget(
            Paragraph::new("  No tracks in this album.").block(block).style(Theme::secondary()),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = tracks.iter().map(|t| {
        let playing = current_id == Some(&t.id);
        let num  = t.track_number.map(|n| format!("{n:>2}. ")).unwrap_or_else(|| "    ".into());
        let dur  = t.duration_secs.map(|s| format!("  {}:{:02}", s/60, s%60)).unwrap_or_default();
        ListItem::new(Line::from(vec![
            Span::styled(num, Theme::muted()),
            Span::styled(if playing { "♪ " } else { "  " }, Theme::accent()),
            Span::styled(&t.title, if playing { Theme::accent_bold() } else { Theme::normal() }),
            Span::styled(dur, Theme::muted()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, state,
    );
}

// ── Songs ─────────────────────────────────────────────────────────────────────

fn draw_songs(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(format!(" Songs ({}) ", app.server.tracks.len()), Theme::accent()));

    if app.server.tracks.is_empty() {
        frame.render_widget(Paragraph::new("  No tracks.").block(block).style(Theme::secondary()), area);
        return;
    }

    let current_id = app.playback.current.clone();

    // Index 0 is the special Shuffle Play entry; tracks start at index 1.
    let shuffle_item = ListItem::new(Line::from(vec![
        Span::styled("  ⇄  Shuffle Play", Theme::accent()),
    ]));

    let mut items: Vec<ListItem> = vec![shuffle_item];
    items.extend(app.server.tracks.iter().map(|t| {
        let playing = current_id.as_ref() == Some(&t.id);
        let dur     = t.duration_secs.map(|s| format!("  {}:{:02}", s/60, s%60)).unwrap_or_default();
        let artist  = t.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled(if playing { "♪ " } else { "  " }, Theme::accent()),
            Span::styled(&t.title, if playing { Theme::accent() } else { Theme::normal() }),
            Span::styled(dur, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }));

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, &mut app.tracks_state,
    );
}

// ── Playlists ─────────────────────────────────────────────────────────────────

fn draw_playlists(frame: &mut Frame, area: Rect, app: &mut App) {
    if let PlaylistView::Tracks { playlist_name, tracks, state } = &mut app.playlist_view {
        draw_playlist_tracks(frame, area, playlist_name, tracks, state, app.playback.current.as_ref());
        return;
    }

    if let PlaylistView::Loading { playlist_name } = &app.playlist_view {
        let pn = playlist_name.clone();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::border_focused())
            .title(Span::styled(format!(" {pn}  — Loading… "), Theme::accent_bold()));
        frame.render_widget(
            Paragraph::new("  Loading playlist tracks…").block(block).style(Theme::muted()),
            area,
        );
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(
            format!(" Playlists ({})  — Enter to expand ", app.playlists.len()),
            Theme::accent(),
        ));

    if app.playlists_loading {
        frame.render_widget(
            Paragraph::new("  Loading playlists…").block(block).style(Theme::muted()),
            area,
        );
        return;
    }

    if !app.playlists_loaded {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  Visit this tab to load your playlists.", Theme::secondary())),
            ])
            .block(block),
            area,
        );
        return;
    }

    if app.playlists.is_empty() {
        frame.render_widget(
            Paragraph::new("  No playlists found.").block(block).style(Theme::secondary()),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app.playlists.iter().map(|p| {
        let count = format!("  {} tracks", p.track_count);
        ListItem::new(Line::from(vec![
            Span::styled(&p.name, Theme::normal()),
            Span::styled(count, Theme::muted()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, &mut app.playlist_state,
    );
}

fn draw_playlist_tracks(
    frame: &mut Frame,
    area: Rect,
    playlist_name: &str,
    tracks: &[Track],
    state: &mut ListState,
    current_id: Option<&TrackId>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(
            format!(" Playlists > {playlist_name}  — Enter play · Esc back "),
            Theme::accent_bold(),
        ));

    if tracks.is_empty() {
        frame.render_widget(
            Paragraph::new("  No tracks in this playlist.").block(block).style(Theme::secondary()),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = tracks.iter().enumerate().map(|(i, t)| {
        let playing = current_id == Some(&t.id);
        let num    = format!("{:>2}. ", i + 1);
        let dur    = t.duration_secs.map(|s| format!("  {}:{:02}", s/60, s%60)).unwrap_or_default();
        let artist = t.artist_name.as_deref().unwrap_or("").to_string();
        ListItem::new(Line::from(vec![
            Span::styled(num, Theme::muted()),
            Span::styled(if playing { "♪ " } else { "  " }, Theme::accent()),
            Span::styled(&t.title, if playing { Theme::accent_bold() } else { Theme::normal() }),
            Span::styled(dur, Theme::muted()),
            Span::raw("  "),
            Span::styled(artist, Theme::secondary()),
        ]))
    }).collect();

    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(Theme::selected()).highlight_symbol("▶ "),
        area, state,
    );
}

// ── Queue ─────────────────────────────────────────────────────────────────────

fn draw_queue(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(" Queue ", Theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.playback.current.is_none() {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(35), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("Nothing playing", Theme::secondary())),
                Line::from(""),
                Line::from(Span::styled(
                    "Navigate to Songs / Artists / Albums / Playlists and press Enter.",
                    Theme::muted(),
                )),
            ])
            .alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    // Now-playing title
    let current = app.queue.current_track();
    let title_line = if let Some(t) = current {
        let artist = t.artist_name.as_deref().unwrap_or("");
        Line::from(vec![
            Span::styled(if app.playback.is_playing { " ▶  " } else { " ⏸  " }, Theme::accent()),
            Span::styled(&t.title, Theme::accent_bold()),
            Span::styled(
                if artist.is_empty() { String::new() } else { format!("  —  {artist}") },
                Theme::secondary(),
            ),
        ])
    } else {
        Line::from(Span::styled(" ▶  (unknown)", Theme::secondary()))
    };
    frame.render_widget(Paragraph::new(vec![Line::from(""), title_line]), sections[0]);

    // Progress gauge
    let pct = if app.playback.duration_secs > 0.0 {
        ((app.playback.position_secs / app.playback.duration_secs) * 100.0).min(100.0) as u16
    } else { 0 };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Theme::accent())
            .percent(pct)
            .label(format!(
                " {}  /  {}",
                fmt_dur(app.playback.position_secs),
                fmt_dur(app.playback.duration_secs),
            )),
        sections[1],
    );

    // Bottom: Up-next + optional Lyrics panel
    if app.current_lyrics.is_some() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(sections[3]);
        draw_upcoming(frame, cols[0], app);
        draw_lyrics_panel(frame, cols[1], app);
    } else {
        draw_upcoming(frame, sections[3], app);
    }
}

fn draw_upcoming(frame: &mut Frame, area: Rect, app: &App) {
    let start_idx = app.queue.current_idx.map(|i| i + 1).unwrap_or(0);
    let items: Vec<ListItem> = app.queue.tracks
        .iter().skip(start_idx).take(30)
        .map(|t| {
            let dur    = t.duration_secs.map(|s| format!(" {}:{:02}", s/60, s%60)).unwrap_or_default();
            let artist = t.artist_name.as_deref().unwrap_or("").to_string();
            ListItem::new(Line::from(vec![
                Span::styled("  ", Theme::muted()),
                Span::styled(&t.title, Theme::normal()),
                Span::styled(dur, Theme::muted()),
                Span::raw("  "),
                Span::styled(artist, Theme::secondary()),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Theme::border())
        .title(Span::styled(" Up next ", Theme::muted()));

    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  End of queue", Theme::muted())).block(block),
            area,
        );
    } else {
        frame.render_widget(List::new(items).block(block), area);
    }
}

fn draw_lyrics_panel(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::LEFT)
        .border_style(Theme::border())
        .title(Span::styled(" Lyrics ", Theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(lyrics) = &app.current_lyrics else { return };
    let height = inner.height as usize;
    let current_idx = lyrics_current_line(lyrics, app.playback.position_secs);

    let window_start = current_idx
        .map(|ci| ci.saturating_sub(2))
        .unwrap_or(0)
        .min(lyrics.lines.len().saturating_sub(height));

    let lines: Vec<Line> = lyrics.lines.iter().enumerate()
        .skip(window_start).take(height)
        .map(|(i, line)| {
            if Some(i) == current_idx {
                Line::from(vec![
                    Span::styled("► ", Theme::accent()),
                    Span::styled(&line.text, Theme::accent_bold()),
                ])
            } else {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(&line.text, Theme::secondary()),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn lyrics_current_line(lyrics: &LyricsData, position_secs: f64) -> Option<usize> {
    if !lyrics.synced || lyrics.lines.is_empty() { return None; }
    let position_ms = (position_secs * 1000.0) as u32;
    let mut current = 0usize;
    for (i, line) in lyrics.lines.iter().enumerate() {
        if let Some(ts) = line.timestamp_ms {
            if ts <= position_ms { current = i; } else { break; }
        }
    }
    Some(current)
}

// ── Settings ──────────────────────────────────────────────────────────────────

fn draw_settings(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .title(Span::styled(" Settings  — j/k navigate  ·  Enter to change ", Theme::accent()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sel = app.settings_selected;
    let ui  = &app.config.ui;

    let row = |idx: usize, label: &str, value: String| -> ListItem<'static> {
        let selected = idx == sel;
        let label = format!("  {label:<28}");
        let value = format!("{value}  ");
        if selected {
            ListItem::new(Line::from(vec![
                Span::styled("▶ ", Theme::accent()),
                Span::styled(label, Theme::selected()),
                Span::styled(value, Theme::accent()),
            ]))
        } else {
            ListItem::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(label, Theme::normal()),
                Span::styled(value, Theme::secondary()),
            ]))
        }
    };

    let tab_name = ui.startup_tab.to_string();
    let items = vec![
        row(0, "Startup Tab", tab_name),
        row(1, "Volume",      format!("{}%  (use +/- to change)", app.playback.volume)),
    ];

    let server_line = app.server.server_name.as_ref()
        .map(|n| format!("Active server: {n}"))
        .unwrap_or_else(|| "No server configured".into());

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(4)])
        .split(inner);

    let mut list_state = ListState::default();
    list_state.select(Some(sel));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Theme::selected()),
        sections[0],
        &mut list_state,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("─".repeat(inner.width as usize), Theme::border())),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(&server_line, Theme::normal()),
            ]),
            Line::from(Span::styled("  Press A to add / change server", Theme::muted())),
        ]),
        sections[1],
    );
}

// ── Search overlay ────────────────────────────────────────────────────────────

fn draw_search_overlay(frame: &mut Frame, search: &SearchState, area: Rect) {
    let modal_area = search_overlay_area(area);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(" Search  (Esc to close) ", Theme::accent_bold()));
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // query input
            Constraint::Length(1), // section selector
            Constraint::Min(0),    // results
            Constraint::Length(1), // hint
        ])
        .split(inner);

    // Query input
    let query_display = format!("  /  {}_", search.query);
    frame.render_widget(
        Paragraph::new(Span::styled(query_display, Theme::normal()))
            .block(Block::default().borders(Borders::BOTTOM).border_style(Theme::border())),
        rows[0],
    );

    // Section tabs
    let section_names = ["Artists", "Albums", "Tracks"];
    let section_counts = [search.artists.len(), search.albums.len(), search.tracks.len()];
    let section_spans: Vec<Span> = section_names.iter().enumerate().flat_map(|(i, &name)| {
        let label  = format!("  {} ({})  ", name, section_counts[i]);
        let style  = if i == search.section { Theme::accent_bold() } else { Theme::muted() };
        let sep    = if i < 2 { Span::styled("│", Theme::muted()) } else { Span::raw("") };
        vec![Span::styled(label, style), sep]
    }).collect();
    frame.render_widget(Paragraph::new(Line::from(section_spans)), rows[1]);

    // Results
    if search.is_searching {
        frame.render_widget(
            Paragraph::new(Span::styled("  Searching…", Theme::muted())),
            rows[2],
        );
    } else if !search.has_results() && !search.query.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  No results. Press Enter to search.", Theme::muted())),
            rows[2],
        );
    } else if !search.has_results() {
        frame.render_widget(
            Paragraph::new(Span::styled("  Type a query and press Enter to search.", Theme::muted())),
            rows[2],
        );
    } else {
        let items: Vec<ListItem> = match search.section {
            0 => search.artists.iter().enumerate().map(|(i, a)| {
                let hi = i == search.selected;
                ListItem::new(Line::from(vec![
                    Span::styled(if hi { "▶ " } else { "  " }, Theme::accent()),
                    Span::styled(&a.name, if hi { Theme::selected() } else { Theme::normal() }),
                    Span::styled(format!("  {} albums", a.album_count), Theme::muted()),
                ]))
            }).collect(),
            1 => search.albums.iter().enumerate().map(|(i, a)| {
                let hi   = i == search.selected;
                let year = a.year.map(|y| format!(" ({y})")).unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(if hi { "▶ " } else { "  " }, Theme::accent()),
                    Span::styled(&a.title, if hi { Theme::selected() } else { Theme::normal() }),
                    Span::styled(year, Theme::muted()),
                    Span::raw("  "),
                    Span::styled(a.artist_name.as_deref().unwrap_or(""), Theme::secondary()),
                ]))
            }).collect(),
            _ => search.tracks.iter().enumerate().map(|(i, t)| {
                let hi  = i == search.selected;
                let dur = t.duration_secs.map(|s| format!(" {}:{:02}", s/60, s%60)).unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(if hi { "▶ " } else { "  " }, Theme::accent()),
                    Span::styled(&t.title, if hi { Theme::selected() } else { Theme::normal() }),
                    Span::styled(dur, Theme::muted()),
                    Span::raw("  "),
                    Span::styled(t.artist_name.as_deref().unwrap_or(""), Theme::secondary()),
                ]))
            }).collect(),
        };
        frame.render_widget(List::new(items), rows[2]);
    }

    // Hint
    let hint = if search.has_results() {
        "  Tab switch section  ·  j/k navigate  ·  Enter play  ·  Esc close"
    } else {
        "  Enter to search  ·  Esc to close"
    };
    frame.render_widget(Paragraph::new(Span::styled(hint, Theme::muted())), rows[3]);
}

fn search_overlay_area(area: Rect) -> Rect {
    let h = (area.height * 85 / 100).max(20);
    let w = (area.width  * 88 / 100).max(60);
    Rect {
        x:      area.x + (area.width  - w) / 2,
        y:      area.y + (area.height - h) / 2,
        width:  w,
        height: h,
    }
}

// ── Help overlay ─────────────────────────────────────────────────────────────

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let w = (area.width  * 70 / 100).max(60).min(area.width);
    let h = (area.height * 80 / 100).max(24).min(area.height);
    let modal = Rect {
        x: area.x + (area.width  - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };

    frame.render_widget(Clear, modal);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::border_focused())
        .title(Span::styled(" Keyboard Shortcuts  (any key to close) ", Theme::accent_bold()));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let col_w = inner.width / 2;
    let left  = Rect { width: col_w, ..inner };
    let right = Rect { x: inner.x + col_w, width: inner.width - col_w, ..inner };

    fn key(k: &'static str, desc: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {k:<14}"), Theme::accent()),
            Span::styled(desc, Theme::normal()),
        ])
    }
    fn header(title: &'static str) -> Line<'static> {
        Line::from(Span::styled(format!("  {title}"), Theme::secondary()))
    }

    let left_lines = vec![
        header("Navigation"),
        key("j / ↓",       "Move down"),
        key("k / ↑",       "Move up"),
        key("Ctrl-d / PgDn","Page down"),
        key("Ctrl-u / PgUp","Page up"),
        key("Enter",        "Select / drill down"),
        key("h / ← / Esc", "Back / up"),
        Line::from(""),
        header("Tabs"),
        key("Tab / Shift-Tab","Next / prev tab"),
        key("1-7",          "Jump to tab"),
        Line::from(""),
        header("Other"),
        key("/",            "Search"),
        key("?",            "This help"),
        key("a",            "Add server"),
        key("q / Ctrl-c",   "Quit"),
    ];

    let right_lines = vec![
        header("Playback"),
        key("Space",        "Play / pause"),
        key("n",            "Next track"),
        key("p",            "Previous track"),
        key("l / →",        "Seek +10 s"),
        key("h / ←",        "Seek -10 s"),
        key("+ / =",        "Volume up"),
        key("-",            "Volume down"),
        key("s",            "Toggle shuffle"),
        key("r",            "Cycle repeat"),
        Line::from(""),
        header("Library"),
        key("Enter (artist)",   "Browse albums"),
        key("Enter (album)",   "Browse tracks"),
        key("Enter (playlist)","Browse tracks"),
    ];

    frame.render_widget(Paragraph::new(left_lines),  left);
    frame.render_widget(Paragraph::new(right_lines), right);
}

// ── Player bar ────────────────────────────────────────────────────────────────

fn draw_player_bar(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).border_style(Theme::border());

    if app.playback.current.is_none() {
        let server_span = if app.server.server_name.is_some() {
            Span::styled(
                format!(" ● {} ", app.server.server_name.as_deref().unwrap_or("")),
                Style::default().fg(Theme::SUCCESS),
            )
        } else {
            Span::styled(" ○ No server ", Style::default().fg(Theme::TEXT_MUTED))
        };

        let right_span = if app.server.is_syncing {
            Span::styled(
                format!(" ⟳ {}  {}/{} ", app.server.sync_label, app.server.sync_done, app.server.sync_total),
                Theme::accent(),
            )
        } else if let Some((msg, is_err, _)) = &app.notification {
            Span::styled(
                format!(" {msg} "),
                Style::default().fg(if *is_err { Theme::ERROR } else { Theme::SUCCESS }),
            )
        } else {
            Span::styled(
                " Enter play  ·  / search  ·  Space pause  ·  n/p skip  ·  +/- vol  ·  q quit ",
                Theme::muted(),
            )
        };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ariafin ", Theme::accent_bold()),
                Span::styled("│", Theme::muted()),
                server_span,
                Span::styled("│", Theme::muted()),
                right_span,
            ])).block(block),
            area,
        );
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let current = app.queue.current_track();
    let (title_str, artist_str) = current
        .map(|t| (t.title.as_str(), t.artist_name.as_deref().unwrap_or("")))
        .unwrap_or(("—", ""));

    let play_icon    = if app.is_loading { "…" } else if app.playback.is_playing { "▶" } else { "⏸" };
    let repeat_icon  = match app.playback.repeat { RepeatMode::Off => "", RepeatMode::All => " ↺", RepeatMode::One => " ↻" };
    let shuffle_icon = if app.playback.shuffle { " ⇄" } else { "" };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {play_icon}  "), Theme::accent()),
            Span::styled(title_str, Theme::accent_bold()),
            Span::styled(
                if artist_str.is_empty() { String::new() } else { format!("  —  {artist_str}") },
                Theme::secondary(),
            ),
            Span::styled(format!("{repeat_icon}{shuffle_icon}"), Theme::muted()),
        ])),
        rows[0],
    );

    // Full-width progress gauge
    let pct = if app.playback.duration_secs > 0.0 {
        ((app.playback.position_secs / app.playback.duration_secs) * 100.0).min(100.0) as u16
    } else { 0 };
    let gauge_label = if app.is_loading {
        " Loading…".to_string()
    } else {
        format!(" {} / {} ", fmt_dur(app.playback.position_secs), fmt_dur(app.playback.duration_secs))
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Theme::accent())
            .percent(pct)
            .label(gauge_label),
        rows[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" vol {}% ", app.playback.volume), Theme::secondary()),
            Span::styled("│", Theme::muted()),
            Span::styled(
                " Space pause  n/p skip  +/- vol  l/h seek  r repeat  s shuffle  / search ",
                Theme::muted(),
            ),
        ])),
        rows[2],
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fmt_dur(secs: f64) -> String {
    let t = secs as u64;
    format!("{}:{:02}", t / 60, t % 60)
}
