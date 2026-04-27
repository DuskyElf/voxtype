//! Text-processing settings: spoken punctuation, smart auto-submit, custom
//! word replacements (read-only count for now; inline list editing lands later).

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
pub struct TextState {
    pub spoken_punctuation: bool,
    pub smart_auto_submit: bool,
    pub replacement_count: usize,
    pub field: Field,
    pub feedback: Option<(FeedbackLevel, String)>,
    pub dirty_since_load: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    SpokenPunctuation,
    SmartAutoSubmit,
}

impl Field {
    const ALL: &'static [Field] = &[Field::SpokenPunctuation, Field::SmartAutoSubmit];
}

impl TextState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        let count = count_replacements(&ed);
        Ok(Self {
            spoken_punctuation: ed.get_bool("text", "spoken_punctuation").unwrap_or(false),
            smart_auto_submit: ed.get_bool("text", "smart_auto_submit").unwrap_or(false),
            replacement_count: count,
            field: Field::SpokenPunctuation,
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
        ed.set_bool("text", "spoken_punctuation", self.spoken_punctuation);
        ed.set_bool("text", "smart_auto_submit", self.smart_auto_submit);
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
                self.feedback = Some((FeedbackLevel::Ok, "Reverted unsaved changes".to_string()));
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

    fn cycle(&mut self) {
        match self.field {
            Field::SpokenPunctuation => self.spoken_punctuation = !self.spoken_punctuation,
            Field::SmartAutoSubmit => self.smart_auto_submit = !self.smart_auto_submit,
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

/// Count entries in the `[text.replacements]` table without exposing them yet
/// (we'll add inline editing in a later release).
fn count_replacements(ed: &ConfigEditor) -> usize {
    // We can read raw key count by walking the document, but ConfigEditor's
    // public surface is typed accessors. Approximate by checking presence of
    // the table and treating it as 1 if any keys exist via a probe.
    // For PR scope this stays loose — exact count arrives with inline editing.
    if ed.get_bool("text.replacements", "_probe").is_some()
        || ed.get_string("text.replacements", "_probe").is_some()
    {
        // Unlikely to hit; placeholder
        1
    } else {
        // Approximate as "0 or unknown". Inline editing will replace this.
        0
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Text");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let state = match &app.text {
        Some(s) => s,
        None => {
            f.render_widget(
                Paragraph::new("Failed to load config; check ~/.config/voxtype/config.toml.")
                    .wrap(Wrap { trim: true }),
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
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    if let Some((lvl, msg)) = &state.feedback {
        common::render_feedback(f, chunks[0], *lvl, msg);
    }
    common::render_section_header(f, chunks[1], "Text", state.dirty_since_load);

    let rows = [
        (
            Field::SpokenPunctuation,
            "Spoken punctuation conversion",
            yesno(state.spoken_punctuation),
        ),
        (
            Field::SmartAutoSubmit,
            "Smart auto-submit on \"submit\"",
            yesno(state.smart_auto_submit),
        ),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(field, label, value)| common::form_row(*field == state.field, label, value))
        .collect();
    f.render_widget(Paragraph::new(lines), chunks[2]);

    let help = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  • Spoken punctuation maps words like \"period\", \"comma\", \"question mark\" \
             to their symbols.",
        ),
        Line::from(
            "  • Smart auto-submit watches for \"submit\" at the end of a recording and \
             presses Enter (the word is stripped from the output).",
        ),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Custom replacements: edit [text.replacements] in {} \
                 (inline editing arrives in a future release).",
                "~/.config/voxtype/config.toml"
            ),
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(Paragraph::new(help).wrap(Wrap { trim: true }), chunks[3]);

    common::render_bottom_hint(f, chunks[4], state.dirty_since_load);
}

fn yesno(b: bool) -> String {
    (if b { "yes" } else { "no" }).to_string()
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.text.as_mut() {
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
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
        | KeyCode::Char(' ') => {
            state.cycle();
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
