use serde::{Deserialize, Serialize};

pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub servers: Vec<ServerConfig>,
    pub active_server: Option<String>,
    pub ui: UiConfig,
    pub playback: PlaybackConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            servers: Vec::new(),
            active_server: None,
            ui: UiConfig::default(),
            playback: PlaybackConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerType {
    Jellyfin,
    Navidrome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// User-defined alias shown in the UI and used as the cache key.
    pub name: String,
    pub server_type: ServerType,
    pub base_url: String,
    pub username: String,
    // Token stored here for Phase 2 convenience; keyring integration in Phase 3.
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub startup_tab: TabId,
    pub layout_density: LayoutDensity,
    pub show_lyrics: bool,
    #[serde(default)]
    pub show_album_art: bool,
    #[serde(default)]
    pub show_visualizer: bool,
    #[serde(default = "default_visualizer_bars")]
    pub visualizer_bars: u8,
}

fn default_visualizer_bars() -> u8 { 20 }

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            startup_tab: TabId::Home,
            layout_density: LayoutDensity::Normal,
            show_lyrics: true,
            show_album_art: false,
            show_visualizer: false,
            visualizer_bars: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TabId {
    Home,
    #[default]
    Artists,
    Albums,
    Songs,
    Playlists,
    Queue,
    Settings,
}

impl std::fmt::Display for TabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TabId::Home => write!(f, "Home"),
            TabId::Artists => write!(f, "Artists"),
            TabId::Albums => write!(f, "Albums"),
            TabId::Songs => write!(f, "Songs"),
            TabId::Playlists => write!(f, "Playlists"),
            TabId::Queue => write!(f, "Queue"),
            TabId::Settings => write!(f, "Settings"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LayoutDensity {
    Compact,
    #[default]
    Normal,
    Comfortable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackConfig {
    pub default_volume: u8,
    pub gapless: bool,
    pub crossfade_duration_ms: u32,
    pub max_bitrate_kbps: Option<u32>,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_volume: 75,
            gapless: true,
            crossfade_duration_ms: 0,
            max_bitrate_kbps: None,
        }
    }
}
