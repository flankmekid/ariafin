use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::types::{
    Album, AlbumId, Artist, ArtistId, CoverArtId, Track, TrackId,
};
use super::cache_db_path;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS cache_meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS artists (
        id          TEXT NOT NULL,
        server_id   TEXT NOT NULL,
        name        TEXT NOT NULL,
        sort_name   TEXT,
        album_count INTEGER NOT NULL DEFAULT 0,
        cover_art_id TEXT,
        PRIMARY KEY (id, server_id)
    );

    CREATE TABLE IF NOT EXISTS albums (
        id          TEXT NOT NULL,
        server_id   TEXT NOT NULL,
        title       TEXT NOT NULL,
        sort_title  TEXT,
        artist_id   TEXT,
        artist_name TEXT,
        year        INTEGER,
        track_count INTEGER NOT NULL DEFAULT 0,
        cover_art_id TEXT,
        PRIMARY KEY (id, server_id)
    );

    CREATE TABLE IF NOT EXISTS tracks (
        id           TEXT NOT NULL,
        server_id    TEXT NOT NULL,
        title        TEXT NOT NULL,
        sort_title   TEXT,
        album_id     TEXT,
        album_title  TEXT,
        artist_id    TEXT,
        artist_name  TEXT,
        track_number INTEGER,
        disc_number  INTEGER,
        duration_secs INTEGER,
        cover_art_id TEXT,
        is_favorite  INTEGER NOT NULL DEFAULT 0,
        play_count   INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (id, server_id)
    );

    CREATE INDEX IF NOT EXISTS idx_artists_name  ON artists(name COLLATE NOCASE);
    CREATE INDEX IF NOT EXISTS idx_albums_artist ON albums(artist_id, server_id);
    CREATE INDEX IF NOT EXISTS idx_tracks_album  ON tracks(album_id, server_id);
    CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist_id, server_id);
    CREATE INDEX IF NOT EXISTS idx_tracks_title  ON tracks(title COLLATE NOCASE);
";

pub struct CacheDb {
    conn: Connection,
    pub server_id: String,
}

impl CacheDb {
    pub fn open(server_id: &str) -> Result<Self> {
        let path = cache_db_path();
        std::fs::create_dir_all(path.parent().expect("db path has no parent"))
            .context("failed to create cache directory")?;

        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open cache db at {}", path.display()))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .context("failed to configure SQLite pragmas")?;
        conn.execute_batch(SCHEMA)
            .context("failed to apply cache schema")?;

        Ok(Self {
            conn,
            server_id: server_id.to_string(),
        })
    }

    // ── Artists ────────────────────────────────────────────────────────────

    pub fn upsert_artists(&self, artists: &[Artist]) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO artists
             (id, server_id, name, sort_name, album_count, cover_art_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for a in artists {
            stmt.execute(params![
                a.id.0,
                self.server_id,
                a.name,
                a.sort_name,
                a.album_count,
                a.cover_art_id,
            ])?;
        }
        Ok(())
    }

    pub fn load_artists(&self) -> Result<Vec<Artist>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, sort_name, album_count, cover_art_id
             FROM artists WHERE server_id = ?1
             ORDER BY COALESCE(sort_name, name) COLLATE NOCASE",
        )?;

        let rows = stmt.query_map(params![self.server_id], |row| {
            Ok(Artist {
                id: ArtistId(row.get(0)?),
                name: row.get(1)?,
                sort_name: row.get(2)?,
                album_count: row.get::<_, u32>(3)?,
                cover_art_id: row.get(4)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load artists from cache")
    }

    // ── Albums ─────────────────────────────────────────────────────────────

    pub fn upsert_albums(&self, albums: &[Album]) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO albums
             (id, server_id, title, sort_title, artist_id, artist_name,
              year, track_count, cover_art_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        for a in albums {
            stmt.execute(params![
                a.id.0,
                self.server_id,
                a.title,
                a.sort_title,
                a.artist_id.as_ref().map(|id| &id.0),
                a.artist_name,
                a.year,
                a.track_count,
                a.cover_art_id,
            ])?;
        }
        Ok(())
    }

    pub fn load_albums(&self) -> Result<Vec<Album>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, sort_title, artist_id, artist_name,
                    year, track_count, cover_art_id
             FROM albums WHERE server_id = ?1
             ORDER BY COALESCE(sort_title, title) COLLATE NOCASE",
        )?;

        let rows = stmt.query_map(params![self.server_id], |row| {
            Ok(Album {
                id: AlbumId(row.get(0)?),
                title: row.get(1)?,
                sort_title: row.get(2)?,
                artist_id: row.get::<_, Option<String>>(3)?.map(ArtistId),
                artist_name: row.get(4)?,
                year: row.get(5)?,
                track_count: row.get::<_, u32>(6)?,
                duration_secs: None,
                cover_art_id: row.get(7)?,
                genre: None,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load albums from cache")
    }

    // ── Tracks ─────────────────────────────────────────────────────────────

    pub fn upsert_tracks(&self, tracks: &[Track]) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO tracks
             (id, server_id, title, sort_title, album_id, album_title,
              artist_id, artist_name, track_number, disc_number,
              duration_secs, cover_art_id, is_favorite, play_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        )?;

        for t in tracks {
            stmt.execute(params![
                t.id.0,
                self.server_id,
                t.title,
                t.sort_title,
                t.album_id.as_ref().map(|id| &id.0),
                t.album_title,
                t.artist_id.as_ref().map(|id| &id.0),
                t.artist_name,
                t.track_number,
                t.disc_number,
                t.duration_secs,
                t.cover_art_id.as_ref().map(|id| &id.0),
                t.is_favorite as i32,
                t.play_count,
            ])?;
        }
        Ok(())
    }

    pub fn load_tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, sort_title, album_id, album_title,
                    artist_id, artist_name, track_number, disc_number,
                    duration_secs, cover_art_id, is_favorite, play_count
             FROM tracks WHERE server_id = ?1
             ORDER BY COALESCE(sort_title, title) COLLATE NOCASE",
        )?;

        let rows = stmt.query_map(params![self.server_id], |row| {
            Ok(Track {
                id: TrackId(row.get(0)?),
                title: row.get(1)?,
                sort_title: row.get(2)?,
                album_id: row.get::<_, Option<String>>(3)?.map(AlbumId),
                album_title: row.get(4)?,
                artist_id: row.get::<_, Option<String>>(5)?.map(ArtistId),
                artist_name: row.get(6)?,
                track_number: row.get(7)?,
                disc_number: row.get(8)?,
                duration_secs: row.get(9)?,
                bitrate: None,
                format: None,
                cover_art_id: row.get::<_, Option<String>>(10)?.map(CoverArtId),
                has_lyrics: false,
                is_favorite: row.get::<_, i32>(11)? != 0,
                play_count: row.get::<_, u32>(12)?,
                last_played_at: None,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load tracks from cache")
    }

    // ── Metadata ───────────────────────────────────────────────────────────

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cache_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT value FROM cache_meta WHERE key = ?1",
        )?;
        let mut rows = stmt.query(params![key])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    pub fn artist_count(&self) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM artists WHERE server_id = ?1",
            params![self.server_id],
            |r| r.get(0),
        )?)
    }
}
