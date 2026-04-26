use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};
use tokio::sync::mpsc;

use af_core::{
    config::Config,
    events::{AudioEvent, BgEvent, PlaybackCommand, UiCommand},
    secrets,
    types::AuthToken,
};
use af_api::{JellyfinClient, MusicServer};

use super::{App, events::map_key, render::draw};

// ── Main run loop ─────────────────────────────────────────────────────────────

pub async fn run(config: Config) -> Result<()> {
    let (cmd_tx, cmd_rx)   = mpsc::channel::<UiCommand>(32);
    let (bg_tx, mut bg_rx) = mpsc::channel::<BgEvent>(64);
    let (pb_tx, pb_rx)     = mpsc::channel::<PlaybackCommand>(32);
    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioEvent>(64);

    tokio::spawn(background_worker(cmd_rx, bg_tx));
    if let Err(e) = af_audio::spawn(pb_rx, audio_tx) {
        tracing::warn!("Failed to start audio engine: {e}");
    }

    if let Some(server_name) = config.active_server.clone() {
        if let Some(srv) = config.servers.iter().find(|s| s.name == server_name) {
            if let Ok(Some((token, user_id))) = secrets::try_get_credentials(&srv.base_url) {
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
            let client = match JellyfinClient::new(&server_name, &base_url) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BgEvent::AuthFailed(format!("Client init failed: {e}"))).await;
                    return;
                }
            };
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
            let client = match JellyfinClient::new(&server_name, &base_url) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BgEvent::SyncFailed(format!("Client init failed: {e}"))).await;
                    return;
                }
            };
            let auth   = AuthToken { token, user_id };
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
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("report start client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            if let Err(e) = client.report_playback_start(&auth, &track_id).await {
                tracing::warn!("report start: {e}");
            }
        }

        UiCommand::ReportPlaybackStop { track_id, position_secs, base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("report stop client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            if let Err(e) = client.report_playback_stop(&auth, &track_id, position_secs).await {
                tracing::warn!("report stop: {e}");
            }
        }

        UiCommand::FetchLyrics { track_id, base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("fetch lyrics client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            let lyrics = client.get_lyrics(&auth, &track_id).await.unwrap_or(None);
            let _ = tx.send(BgEvent::LyricsLoaded(lyrics)).await;
        }

        UiCommand::LoadPlaylists { base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("load playlists client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            match client.get_playlists(&auth).await {
                Ok(playlists) => { let _ = tx.send(BgEvent::PlaylistsLoaded(playlists)).await; }
                Err(e)        => tracing::warn!("load playlists: {e}"),
            }
        }

        UiCommand::LoadPlaylistTracks { playlist_id, base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("load playlist tracks client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            match client.get_playlist_tracks(&auth, &playlist_id).await {
                Ok(tracks) => { let _ = tx.send(BgEvent::PlaylistTracksReady(tracks)).await; }
                Err(e)     => tracing::warn!("load playlist tracks: {e}"),
            }
        }

        UiCommand::FetchHomeData { base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("fetch home data client init: {e}"); return; }
            };
            let auth   = AuthToken { token, user_id };
            let recently_added  = client.get_recently_added(&auth, 10).await.unwrap_or_default();
            let recently_played = client.get_recently_played(&auth, 10).await.unwrap_or_default();
            let _ = tx.send(BgEvent::HomeDataLoaded { recently_added, recently_played }).await;
        }

        UiCommand::Search { query, base_url, token, user_id } => {
            let client = match JellyfinClient::new("", &base_url) {
                Ok(c) => c,
                Err(e) => { tracing::warn!("search client init: {e}"); return; }
            };
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
