use af_core::types::{Album, Artist, Track};

/// Library data loaded from the SQLite cache.
#[derive(Default)]
pub struct ServerState {
    pub server_name: Option<String>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub is_syncing: bool,
    pub sync_label: String,
    pub sync_done: u32,
    pub sync_total: u32,
}

impl ServerState {
    pub fn sync_pct(&self) -> u16 {
        if self.sync_total == 0 { return 0; }
        ((self.sync_done as f32 / self.sync_total as f32) * 100.0) as u16
    }
}

/// Focused field in the login/add-server modal.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginField {
    #[default]
    Url,
    Username,
    Password,
}

impl LoginField {
    pub fn next(self) -> Self {
        match self { Self::Url => Self::Username, Self::Username => Self::Password, Self::Password => Self::Url }
    }
    pub fn prev(self) -> Self {
        match self { Self::Url => Self::Password, Self::Password => Self::Username, Self::Username => Self::Url }
    }
}

/// State for the add-server / login modal.
pub struct LoginModal {
    pub url: String,
    pub username: String,
    pub password: String,
    pub focused: LoginField,
    pub error: Option<String>,
    pub submitting: bool,
}

impl Default for LoginModal {
    fn default() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            focused: LoginField::Url,
            error: None,
            submitting: false,
        }
    }
}

impl LoginModal {
    pub fn focused_field_mut(&mut self) -> &mut String {
        match self.focused {
            LoginField::Url      => &mut self.url,
            LoginField::Username => &mut self.username,
            LoginField::Password => &mut self.password,
        }
    }
}
