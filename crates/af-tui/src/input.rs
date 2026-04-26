use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Top-level actions dispatched from key presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    TabNext,
    TabPrev,
    TabGoto(usize),
    ScrollDown,
    ScrollUp,
    ScrollPageDown,
    ScrollPageUp,
    Enter,
    Back,
    TogglePause,
    NextTrack,
    PrevTrack,
    VolumeUp,
    VolumeDown,
    SeekForward,
    SeekBackward,
    ToggleShuffle,
    CycleRepeat,
    SearchOpen,
    HelpOpen,
    CharInput(char),
    Backspace,
    None,
}

pub fn map_key(key: KeyEvent) -> Action {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
        KeyCode::Char('c') if ctrl => Action::Quit,

        // Tab navigation
        KeyCode::Tab => Action::TabNext,
        KeyCode::BackTab => Action::TabPrev,
        KeyCode::Char('1') => Action::TabGoto(0),
        KeyCode::Char('2') => Action::TabGoto(1),
        KeyCode::Char('3') => Action::TabGoto(2),
        KeyCode::Char('4') => Action::TabGoto(3),
        KeyCode::Char('5') => Action::TabGoto(4),
        KeyCode::Char('6') => Action::TabGoto(5),
        KeyCode::Char('7') => Action::TabGoto(6),

        // List navigation
        KeyCode::Char('j') | KeyCode::Down  => Action::ScrollDown,
        KeyCode::Char('k') | KeyCode::Up    => Action::ScrollUp,
        KeyCode::Char('d') if ctrl => Action::ScrollPageDown,
        KeyCode::Char('u') if ctrl => Action::ScrollPageUp,
        KeyCode::PageDown => Action::ScrollPageDown,
        KeyCode::PageUp   => Action::ScrollPageUp,
        KeyCode::Enter    => Action::Enter,
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc | KeyCode::Backspace => Action::Back,

        // Playback
        KeyCode::Char(' ') => Action::TogglePause,
        KeyCode::Char('n') => Action::NextTrack,
        KeyCode::Char('p') => Action::PrevTrack,
        KeyCode::Char('=') | KeyCode::Char('+') => Action::VolumeUp,
        KeyCode::Char('-') => Action::VolumeDown,
        KeyCode::Char('l') | KeyCode::Right => Action::SeekForward,
        KeyCode::Char('s') => Action::ToggleShuffle,
        KeyCode::Char('r') => Action::CycleRepeat,

        _ => Action::None,
    }
}
