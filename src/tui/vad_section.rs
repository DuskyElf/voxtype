//! Voice Activity Detection settings.

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
pub struct VadState {
    pub enabled: bool,
    pub backend: String,
    pub threshold: f32,
    pub field: Field,
    pub feedback: Option<(FeedbackLevel, String)>,
    pub dirty_since_load: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Enabled,
    Backend,
    Threshold,
}
impl Field {
    const ALL: &'static [Field] = &[Field::Enabled, Field::Backend, Field::Threshold];
}
const BACKEND_CHOICES: &[&str] = &["auto", "energy", "whisper"];

impl VadState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        Ok(Self {
            enabled: ed.get_bool("vad", "enabled").unwrap_or(false),
            backend: ed
                .get_string("vad", "backend")
                .unwrap_or_else(|| "auto".to_string()),
            threshold: ed
                .get_string("vad", "threshold")
                .and_then(|s| s.parse().ok())
                .or_else(|| ed.get_int("vad", "threshold").map(|n| n as f32))
                .unwrap_or(0.5),
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
        ed.set_bool("vad", "enabled", self.enabled);
        ed.set_string("vad", "backend", &self.backend);
        ed.set_string("vad", "threshold", &format!("{:.2}", self.threshold));
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
        match Self::load() {
            Ok(fresh) => {
                let field = self.field;
                *self = fresh;
                self.field = field;
                self.feedback = Some((FeedbackLevel::Ok, "Reverted".to_string()));
            }
            Err(e) => self.feedback = Some((FeedbackLevel::Err, format!("reload: {}", e))),
        }
    }

    fn move_field(&mut self, delta: i32) {
        let len = Field::ALL.len() as i32;
        let cur = Field::ALL.iter().position(|f| *f == self.field).unwrap_or(0) as i32;
        let new = (cur + delta).rem_euclid(len);
        self.field = Field::ALL[new as usize];
    }

    fn cycle(&mut self, delta: i32) {
        match self.field {
            Field::Enabled => self.enabled = !self.enabled,
            Field::Backend => {
                let idx = BACKEND_CHOICES
                    .iter()
                    .position(|c| *c == self.backend)
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(BACKEND_CHOICES.len() as i32);
                self.backend = BACKEND_CHOICES[n as usize].to_string();
            }
            Field::Threshold => {
                self.threshold = (self.threshold + delta as f32 * 0.05).clamp(0.0, 1.0);
            }
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("VAD");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let state = match &app.vad {
        Some(s) => s,
        None => {
            f.render_widget(
                Paragraph::new("Failed to load config.").wrap(Wrap { trim: true }),
                inner,
            );
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
    common::render_section_header(f, chunks[1], "Voice Activity Detection", state.dirty_since_load);

    let rows = [
        (Field::Enabled, "Enabled", yesno(state.enabled)),
        (Field::Backend, "Backend", state.backend.clone()),
        (
            Field::Threshold,
            "Speech threshold",
            format!("{:.2}", state.threshold),
        ),
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
            "  • VAD filters silence-only recordings before transcription, \
             preventing Whisper from hallucinating phrases on quiet input.",
        ),
        Line::from(
            "  • backend = auto picks Whisper VAD for the Whisper engine \
             (downloads ggml-silero-vad.bin) and Energy VAD for ONNX engines \
             (no model needed).",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "  Run `voxtype setup vad` to download the Silero VAD model.",
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
    let state = match app.vad.as_mut() {
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
