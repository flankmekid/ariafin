use serde::{Deserialize, Serialize};
use crate::types::TrackId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

impl std::fmt::Display for RepeatMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::All => write!(f, "All"),
            Self::One => write!(f, "One"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackState {
    pub current: Option<TrackId>,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub volume: u8,
    pub is_playing: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}
