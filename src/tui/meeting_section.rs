//! Meeting mode settings: enabled, audio source, diarization on/off.

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
pub struct MeetingState {
    pub enabled: bool,
    pub diarization_enabled: bool,
    pub audio_source: String,
    pub field: Field,
    pub feedback: Option<(FeedbackLevel, String)>,
    pub dirty_since_load: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Enabled,
    Diarization,
    AudioSource,
}
impl Field {
    const ALL: &'static [Field] = &[Field::Enabled, Field::Diarization, Field::AudioSource];
}
const SOURCES: &[&str] = &["mic", "system", "both"];

impl MeetingState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        Ok(Self {
            enabled: ed.get_bool("meeting", "enabled").unwrap_or(false),
            diarization_enabled: ed
                .get_bool("meeting.diarization", "enabled")
                .unwrap_or(false),
            audio_source: ed
                .get_string("meeting.audio", "source")
                .unwrap_or_else(|| "mic".to_string()),
            field: Field::Enabled,
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
        ed.set_bool("meeting", "enabled", self.enabled);
        ed.set_bool("meeting.diarization", "enabled", self.diarization_enabled);
        ed.set_string("meeting.audio", "source", &self.audio_source);
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
            let field = self.field;
            *self = fresh;
            self.field = field;
            self.feedback = Some((FeedbackLevel::Ok, "Reverted".to_string()));
        }
    }
    fn move_field(&mut self, delta: i32) {
        let len = Field::ALL.len() as i32;
        let cur = Field::ALL.iter().position(|f| *f == self.field).unwrap_or(0) as i32;
        self.field = Field::ALL[((cur + delta).rem_euclid(len)) as usize];
    }
    fn cycle(&mut self, delta: i32) {
        match self.field {
            Field::Enabled => self.enabled = !self.enabled,
            Field::Diarization => self.diarization_enabled = !self.diarization_enabled,
            Field::AudioSource => {
                let idx = SOURCES
                    .iter()
                    .position(|s| *s == self.audio_source)
                    .map(|i| i as i32)
                    .unwrap_or(0);
                self.audio_source = SOURCES
                    [(idx + delta).rem_euclid(SOURCES.len() as i32) as usize]
                    .to_string();
            }
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Meeting");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let state = match &app.meeting {
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
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
    if let Some((lvl, msg)) = &state.feedback {
        common::render_feedback(f, chunks[0], *lvl, msg);
    }
    common::render_section_header(f, chunks[1], "Meeting Mode", state.dirty_since_load);
    let rows = [
        (Field::Enabled, "Enabled", yesno(state.enabled)),
        (
            Field::Diarization,
            "Speaker diarization",
            yesno(state.diarization_enabled),
        ),
        (Field::AudioSource, "Audio source", state.audio_source.clone()),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(f, l, v)| common::form_row(*f == state.field, l, v))
        .collect();
    f.render_widget(Paragraph::new(lines), chunks[2]);

    let help = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  • Meeting mode chunks long recordings, persists segments, and \
             optionally attributes them to speakers.",
        ),
        Line::from(
            "  • audio_source = both captures mic + system loopback for \
             two-sided meeting transcripts (uses GTCRN echo cancellation).",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "Edit chunk size, summary, and per-speaker config in [meeting.*] in \
             config.toml directly.",
            Style::default().fg(Color::Gray),
        )),
    ];
    f.render_widget(Paragraph::new(help).wrap(Wrap { trim: true }), chunks[3]);
    common::render_bottom_hint(f, chunks[4], state.dirty_since_load);
}
fn yesno(b: bool) -> String {
    (if b { "yes" } else { "no" }).to_string()
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.meeting.as_mut() {
        Some(s) => s,
        None => return Action::None,
    };
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_field(-1);
            Action::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_field(1);
            Action::None
        }
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
