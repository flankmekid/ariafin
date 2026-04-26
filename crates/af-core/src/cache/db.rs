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
        let path = cache_db_path()?;
        std::fs::create_dir_all(path.parent()
            .ok_or_else(|| anyhow::anyhow!("Database path has no parent directory"))?)
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AlbumId, ArtistId, CoverArtId, TrackId};
    use rusqlite::Connection;

    fn open_in_memory(server_id: &str) -> CacheDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        CacheDb {
            conn,
            server_id: server_id.to_string(),
        }
    }

    fn sample_artist(id: &str, name: &str) -> Artist {
        Artist {
            id: ArtistId(id.to_string()),
            name: name.to_string(),
            sort_name: None,
            album_count: 1,
            cover_art_id: None,
        }
    }

    fn sample_album(id: &str, title: &str, artist_id: Option<&str>) -> Album {
        Album {
            id: AlbumId(id.to_string()),
            title: title.to_string(),
            sort_title: None,
            artist_id: artist_id.map(|s| ArtistId(s.to_string())),
            artist_name: artist_id.map(|_| "Test Artist".to_string()),
            year: Some(2024),
            track_count: 2,
            duration_secs: None,
            cover_art_id: None,
            genre: None,
        }
    }

    fn sample_track(id: &str, title: &str, album_id: Option<&str>, artist_id: Option<&str>) -> Track {
        Track {
            id: TrackId(id.to_string()),
            title: title.to_string(),
            sort_title: None,
            album_id: album_id.map(|s| AlbumId(s.to_string())),
            album_title: album_id.map(|_| "Test Album".to_string()),
            artist_id: artist_id.map(|s| ArtistId(s.to_string())),
            artist_name: artist_id.map(|_| "Test Artist".to_string()),
            disc_number: Some(1),
            track_number: Some(1),
            duration_secs: Some(180),
            bitrate: None,
            format: None,
            cover_art_id: Some(CoverArtId("cover-1".to_string())),
            has_lyrics: false,
            play_count: 0,
            last_played_at: None,
            is_favorite: false,
        }
    }

    #[test]
    fn test_artist_upsert_and_load() {
        let db = open_in_memory("srv1");
        let artists = vec![
            sample_artist("a1", "Alpha"),
            sample_artist("a2", "Beta"),
        ];

        db.upsert_artists(&artists).unwrap();
        let loaded = db.load_artists().unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id.0, "a1");
        assert_eq!(loaded[1].id.0, "a2");
        assert_eq!(db.artist_count().unwrap(), 2);
    }

    #[test]
    fn test_album_upsert_and_load_with_foreign_key() {
        let db = open_in_memory("srv1");
        let artists = vec![sample_artist("a1", "Artist One")];
        let albums = vec![
            sample_album("al1", "Album One", Some("a1")),
            sample_album("al2", "Album Two", None),
        ];

        db.upsert_artists(&artists).unwrap();
        db.upsert_albums(&albums).unwrap();
        let loaded = db.load_albums().unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id.0, "al1");
        assert_eq!(loaded[0].artist_id.as_ref().unwrap().0, "a1");
        assert_eq!(loaded[0].artist_name.as_ref().unwrap(), "Test Artist");
        assert!(loaded[1].artist_id.is_none());
    }

    #[test]
    fn test_track_upsert_and_load_with_relationships() {
        let db = open_in_memory("srv1");
        let artists = vec![sample_artist("a1", "Artist One")];
        let albums = vec![sample_album("al1", "Album One", Some("a1"))];
        let tracks = vec![
            sample_track("t1", "Track One", Some("al1"), Some("a1")),
            sample_track("t2", "Track Two", None, None),
        ];

        db.upsert_artists(&artists).unwrap();
        db.upsert_albums(&albums).unwrap();
        db.upsert_tracks(&tracks).unwrap();
        let loaded = db.load_tracks().unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id.0, "t1");
        assert_eq!(loaded[0].album_id.as_ref().unwrap().0, "al1");
        assert_eq!(loaded[0].artist_id.as_ref().unwrap().0, "a1");
        assert_eq!(loaded[0].duration_secs, Some(180));
        assert!(loaded[1].album_id.is_none());
        assert!(loaded[1].artist_id.is_none());
    }

    #[test]
    fn test_server_isolation() {
        let db1 = open_in_memory("srv1");
        let db2 = open_in_memory("srv2");

        db1.upsert_artists(&[sample_artist("a1", "Srv1 Artist")]).unwrap();
        db2.upsert_artists(&[sample_artist("a1", "Srv2 Artist")]).unwrap();

        let loaded1 = db1.load_artists().unwrap();
        let loaded2 = db2.load_artists().unwrap();

        assert_eq!(loaded1.len(), 1);
        assert_eq!(loaded1[0].name, "Srv1 Artist");
        assert_eq!(loaded2.len(), 1);
        assert_eq!(loaded2[0].name, "Srv2 Artist");
    }

    #[test]
    fn test_meta_roundtrip() {
        let db = open_in_memory("srv1");
        db.set_meta("last_sync", "2024-01-01T00:00:00Z").unwrap();
        let val = db.get_meta("last_sync").unwrap();
        assert_eq!(val, Some("2024-01-01T00:00:00Z".to_string()));

        db.set_meta("last_sync", "2024-02-01T00:00:00Z").unwrap();
        let val = db.get_meta("last_sync").unwrap();
        assert_eq!(val, Some("2024-02-01T00:00:00Z".to_string()));
    }

    #[test]
    fn test_upsert_overwrites_existing() {
        let db = open_in_memory("srv1");
        let artist1 = sample_artist("a1", "Original");
        let artist2 = sample_artist("a1", "Updated");

        db.upsert_artists(&[artist1]).unwrap();
        db.upsert_artists(&[artist2]).unwrap();

        let loaded = db.load_artists().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Updated");
    }
}
