use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};

use af_core::types::{LyricsData, RepeatMode, Track, TrackId};
use crate::theme::Theme;
use crate::widgets::draw_login_modal;
use super::{App, ArtistView, AlbumView, PlaylistView, SearchState, TABS};

// ── Drawing ───────────────────────────────────────────────────────────────────

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
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

fn draw_artist_albums(frame: &mut Frame, area: Rect, artist_name: &str, albums: &[af_core::types::Album], state: &mut ListState) {
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
