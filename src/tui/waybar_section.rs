//! Waybar status integration: icon theme.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::{Action, App};
use super::common::{self, FeedbackLevel};
use super::config_editor::{ConfigEditor, EditorError};

#[derive(Debug, Clone)]
pub struct WaybarState {
    pub icon_theme: String,
    pub feedback: Option<(FeedbackLevel, String)>,
    pub dirty_since_load: bool,
}

const THEMES: &[&str] = &[
    "emoji",
    "nerd-font",
    "material",
    "phosphor",
    "codicons",
    "omarchy",
    "minimal",
    "dots",
    "arrows",
    "text",
];

impl WaybarState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        Ok(Self {
            icon_theme: ed
                .get_string("status", "icon_theme")
                .unwrap_or_else(|| "emoji".to_string()),
            feedback: None,
            dirty_since_load: false,
        })
    }
    pub fn save(&mut self) -> Action {
        let mut ed = match ConfigEditor::load() {
            Ok(e) => e,
            Err(e) => {
                self.feedback = Some((FeedbackLevel::Err, format!("load: {}", e)));
                return Action::None;
            }
        };
        ed.set_string("status", "icon_theme", &self.icon_theme);
        match ed.save() {
            Ok(()) => {
                self.dirty_since_load = false;
                self.feedback = Some((
                    FeedbackLevel::Ok,
                    format!("Saved to {}", ed.path().display()),
                ));
            }
            Err(e) => self.feedback = Some((FeedbackLevel::Err, format!("save: {}", e))),
        }
        Action::None
    }
    pub fn reset(&mut self) {
        if let Ok(fresh) = Self::load() {
            *self = fresh;
            self.feedback = Some((FeedbackLevel::Ok, "Reverted".to_string()));
        }
    }
    fn cycle(&mut self, delta: i32) {
        let idx = THEMES
            .iter()
            .position(|t| *t == self.icon_theme)
            .map(|i| i as i32)
            .unwrap_or(0);
        self.icon_theme = THEMES[((idx + delta).rem_euclid(THEMES.len() as i32)) as usize].to_string();
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Waybar");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let state = match &app.waybar {
        Some(s) => s,
        None => {
            f.render_widget(Paragraph::new("Failed to load config."), inner);
            return;
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if state.feedback.is_some() { 2 } else { 0 }),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    if let Some((lvl, msg)) = &state.feedback {
        common::render_feedback(f, chunks[0], *lvl, msg);
    }
    common::render_section_header(f, chunks[1], "Waybar / Status", state.dirty_since_load);
    let row = common::form_row(true, "Icon theme", &state.icon_theme);
    f.render_widget(Paragraph::new(vec![row]), chunks[2]);

    let help = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  • Theme picks the glyph set used by `voxtype status --follow`. \
             Match it to your status bar font (nerd-font for Nerd Fonts, \
             phosphor for Phosphor icons, omarchy for the Omarchy ricing).",
        ),
        Line::from(
            "  • Per-state overrides live under [status.icons] in config.toml \
             (e.g. icons.recording = \"●\"). Inline editing of overrides is \
             not yet in the TUI.",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "  Run `voxtype setup waybar` for ready-to-paste Waybar config.",
            Style::default().fg(Color::Gray),
        )),
    ];
    f.render_widget(Paragraph::new(help).wrap(Wrap { trim: true }), chunks[3]);
    common::render_bottom_hint(f, chunks[4], state.dirty_since_load);
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.waybar.as_mut() {
        Some(s) => s,
        None => return Action::None,
    };
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.cycle(-1);
            Action::None
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
            state.cycle(1);
            Action::None
        }
        KeyCode::Char('s') => state.save(),
        KeyCode::Char('r') => {
            state.reset();
            Action::None
        }
        _ => Action::None,
    }
}
