use std::time::{Duration, Instant};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::widgets::ListState;

use af_core::{
    config::{loader as cfg_loader, schema::ServerConfig},
    events::{AudioEvent, BgEvent, PlaybackCommand, UiCommand},
    secrets,
    types::{Album, Track},
};
use crate::input::Action;
use crate::state::LoginModal;
use super::{App, ArtistView, AlbumView, PlaylistView, SearchState, TABS};

// ── Key mapping ───────────────────────────────────────────────────────────────

pub(super) fn map_key(code: KeyCode, mods: KeyModifiers) -> Action {
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

// ── Input handling ────────────────────────────────────────────────────────────

impl App {
    pub(super) async fn handle(&mut self, action: Action) -> bool {
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
                search.section  = (search.section + 1) % 3;
                search.selected = 0;
                search.clamp_selected();
            }
            Action::TabPrev => {
                search.section  = if search.section == 0 { 2 } else { search.section - 1 };
                search.selected = 0;
                search.clamp_selected();
            }

            Action::ScrollDown => {
                let len = search.section_len();
                if len > 0 { search.selected = (search.selected + 1).min(len - 1); }
            }
            Action::ScrollUp => {
                search.selected = search.selected.saturating_sub(1);
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

    pub(super) fn handle_bg_event(&mut self, event: BgEvent) {
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

                if self.config.servers.iter().find(|s| s.name == server_name).is_none() {
                    self.config.servers.push(ServerConfig {
                        name: server_name.clone(),
                        server_type: af_core::config::schema::ServerType::Jellyfin,
                        base_url: base_url.clone(),
                        username: modal_username,
                    });
                    self.config.active_server = Some(server_name.clone());
                }

                // Store credentials in system keyring
                if let Err(e) = secrets::store_credentials(&base_url, &user_id, &token) {
                    tracing::warn!("Failed to store credentials in keyring for {}: {}", base_url, e);
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

    pub(super) fn handle_audio_event(&mut self, event: AudioEvent) {
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
