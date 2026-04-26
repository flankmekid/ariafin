use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use crate::theme::Theme;
use crate::state::{LoginField, LoginModal};

/// Render a centred overlay modal box.
pub fn modal_area(frame_area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_pct) / 2),
            Constraint::Percentage(height_pct),
            Constraint::Percentage((100 - height_pct) / 2),
        ])
        .split(frame_area);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_pct) / 2),
            Constraint::Percentage(width_pct),
            Constraint::Percentage((100 - width_pct) / 2),
        ])
        .split(vert[1]);
    horiz[1]
}

/// Render the login / add-server modal.
pub fn draw_login_modal(frame: &mut Frame, modal: &LoginModal) {
    let area = modal_area(frame.area(), 50, 60);
    frame.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Theme::accent())
        .title(Span::styled(" Add Server ", Theme::accent_bold()));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top padding
            Constraint::Length(3), // URL
            Constraint::Length(1), // gap
            Constraint::Length(3), // Username
            Constraint::Length(1), // gap
            Constraint::Length(3), // Password
            Constraint::Length(1), // gap
            Constraint::Length(1), // hint / error
            Constraint::Min(0),
        ])
        .split(inner);

    draw_field(frame, rows[1], "Server URL  (e.g. http://192.168.1.1:8096)", &modal.url, modal.focused == LoginField::Url, false);
    draw_field(frame, rows[3], "Username",   &modal.username, modal.focused == LoginField::Username, false);
    draw_field(frame, rows[5], "Password",   &modal.password, modal.focused == LoginField::Password, true);

    // Hint / error line
    let hint = if modal.submitting {
        Span::styled(" Authenticating…", Theme::accent())
    } else if let Some(e) = &modal.error {
        Span::styled(format!(" ✗ {e}"), Style::default().fg(Theme::ERROR))
    } else {
        Span::styled(" Tab next field  ·  Enter submit  ·  Esc cancel", Theme::muted())
    };
    frame.render_widget(Paragraph::new(Line::from(hint)).alignment(Alignment::Center), rows[7]);
}

fn draw_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    masked: bool,
) {
    let border_style = if focused { Theme::border_focused() } else { Theme::border() };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {label} "), if focused { Theme::accent() } else { Theme::muted() }));

    let display = if masked {
        "•".repeat(value.len())
    } else {
        value.to_string()
    };

    // Show cursor as trailing block character when focused
    let text = if focused {
        format!("{display}▌")
    } else {
        display
    };

    let para = Paragraph::new(text)
        .block(block)
        .style(Theme::normal());

    frame.render_widget(para, area);
}
