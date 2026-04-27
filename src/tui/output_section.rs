//! Output section: how transcribed text is delivered.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::app::{Action, App};
use super::config_editor::{ConfigEditor, EditorError};

#[derive(Debug, Clone)]
pub struct OutputState {
    pub mode: String,
    pub fallback_to_clipboard: bool,
    pub auto_submit: bool,
    pub shift_enter_newlines: bool,
    pub pre_type_delay_ms: i64,
    pub append_text: Option<String>,
    pub post_process_command: Option<String>,
    pub field: Field,
    pub feedback: Option<Feedback>,
    pub dirty_since_load: bool,
}

#[derive(Debug, Clone)]
pub struct Feedback {
    pub level: FeedbackLevel,
    pub message: String,
}
#[derive(Debug, Clone, Copy)]
pub enum FeedbackLevel {
    Ok,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Mode,
    Fallback,
    AutoSubmit,
    ShiftEnterNewlines,
    PreTypeDelay,
    AppendText,
    PostProcess,
}

impl Field {
    const ALL: &'static [Field] = &[
        Field::Mode,
        Field::Fallback,
        Field::AutoSubmit,
        Field::ShiftEnterNewlines,
        Field::PreTypeDelay,
        Field::AppendText,
        Field::PostProcess,
    ];
}

const MODE_CHOICES: &[&str] = &["type", "clipboard", "paste", "file"];
const APPEND_CHOICES: &[Option<&str>] = &[None, Some(" "), Some("\n"), Some(". ")];
const POST_PROCESS_PRESETS: &[Option<&str>] = &[
    None,
    Some("ollama run llama3.2 'Polish: '"),
    Some("sed 's/uh, //g'"),
];
const DELAY_STEP: i64 = 25;

impl OutputState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        Ok(Self {
            mode: ed
                .get_string("output", "mode")
                .unwrap_or_else(|| "type".to_string()),
            fallback_to_clipboard: ed
                .get_bool("output", "fallback_to_clipboard")
                .unwrap_or(true),
            auto_submit: ed.get_bool("output", "auto_submit").unwrap_or(false),
            shift_enter_newlines: ed
                .get_bool("output", "shift_enter_newlines")
                .unwrap_or(false),
            pre_type_delay_ms: ed.get_int("output", "pre_type_delay_ms").unwrap_or(0),
            append_text: ed.get_string("output", "append_text"),
            post_process_command: ed.get_string("post_process", "command"),
            field: Field::Mode,
            feedback: None,
            dirty_since_load: false,
        })
    }

    pub fn save(&mut self) -> Action {
        let mut ed = match ConfigEditor::load() {
            Ok(e) => e,
            Err(e) => {
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Err,
                    message: format!("load: {}", e),
                });
                return Action::None;
            }
        };
        ed.set_string("output", "mode", &self.mode);
        ed.set_bool(
            "output",
            "fallback_to_clipboard",
            self.fallback_to_clipboard,
        );
        ed.set_bool("output", "auto_submit", self.auto_submit);
        ed.set_bool("output", "shift_enter_newlines", self.shift_enter_newlines);
        ed.set_int("output", "pre_type_delay_ms", self.pre_type_delay_ms);
        match &self.append_text {
            Some(t) => ed.set_string("output", "append_text", t),
            None => ed.unset("output", "append_text"),
        }
        match &self.post_process_command {
            Some(c) if !c.is_empty() => ed.set_string("post_process", "command", c),
            _ => ed.unset("post_process", "command"),
        }
        match ed.save() {
            Ok(()) => {
                self.dirty_since_load = false;
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Ok,
                    message: format!("Saved to {}", ed.path().display()),
                });
            }
            Err(e) => {
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Err,
                    message: format!("save: {}", e),
                });
            }
        }
        Action::None
    }

    pub fn reset(&mut self) {
        match Self::load() {
            Ok(fresh) => {
                let field = self.field;
                *self = fresh;
                self.field = field;
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Ok,
                    message: "Reverted unsaved changes".to_string(),
                });
            }
            Err(e) => {
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Err,
                    message: format!("reload: {}", e),
                });
            }
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
            Field::Mode => {
                let idx = MODE_CHOICES
                    .iter()
                    .position(|c| *c == self.mode)
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(MODE_CHOICES.len() as i32);
                self.mode = MODE_CHOICES[n as usize].to_string();
            }
            Field::Fallback => self.fallback_to_clipboard = !self.fallback_to_clipboard,
            Field::AutoSubmit => self.auto_submit = !self.auto_submit,
            Field::ShiftEnterNewlines => self.shift_enter_newlines = !self.shift_enter_newlines,
            Field::PreTypeDelay => {
                self.pre_type_delay_ms =
                    (self.pre_type_delay_ms + delta as i64 * DELAY_STEP).clamp(0, 5000);
            }
            Field::AppendText => {
                let idx = APPEND_CHOICES
                    .iter()
                    .position(|c| c.as_deref() == self.append_text.as_deref())
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(APPEND_CHOICES.len() as i32);
                self.append_text = APPEND_CHOICES[n as usize].map(|s| s.to_string());
            }
            Field::PostProcess => {
                let idx = POST_PROCESS_PRESETS
                    .iter()
                    .position(|c| c.as_deref() == self.post_process_command.as_deref())
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(POST_PROCESS_PRESETS.len() as i32);
                self.post_process_command =
                    POST_PROCESS_PRESETS[n as usize].map(|s| s.to_string());
            }
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Output");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let state = match &app.output {
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
            Constraint::Length(2), // header
            Constraint::Length(9), // form
            Constraint::Min(0),    // help
            Constraint::Length(1), // hint
        ])
        .split(inner);

    if let Some(fb) = &state.feedback {
        super::common::render_feedback(f, chunks[0], fb.level.into(), &fb.message);
    }
    super::common::render_section_header(
        f,
        chunks[1],
        "Output",
        state.dirty_since_load,
    );
    render_form(f, chunks[2], state);
    render_help(f, chunks[3]);
    super::common::render_bottom_hint(f, chunks[4], state.dirty_since_load);
}

fn render_form(f: &mut Frame, area: Rect, state: &OutputState) {
    let rows = [
        (Field::Mode, "Output mode", state.mode.clone()),
        (
            Field::Fallback,
            "Fallback to clipboard",
            yesno(state.fallback_to_clipboard),
        ),
        (
            Field::AutoSubmit,
            "Auto-submit (press Enter)",
            yesno(state.auto_submit),
        ),
        (
            Field::ShiftEnterNewlines,
            "Newlines as Shift+Enter",
            yesno(state.shift_enter_newlines),
        ),
        (
            Field::PreTypeDelay,
            "Pre-type delay (ms)",
            state.pre_type_delay_ms.to_string(),
        ),
        (
            Field::AppendText,
            "Append after each",
            display_append(state.append_text.as_deref()),
        ),
        (
            Field::PostProcess,
            "Post-process command",
            state
                .post_process_command
                .as_deref()
                .map(|s| {
                    if s.len() > 36 {
                        format!("{}…", &s[..36])
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| "(none)".to_string()),
        ),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(field, label, value)| super::common::form_row(*field == state.field, label, value))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn render_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  • type uses wtype -> dotool -> ydotool. clipboard puts text on \
             the clipboard only. paste = clipboard + Ctrl+V. file appends to \
             a file path you set in [output] file_path.",
        ),
        Line::from(
            "  • Auto-submit hits Enter automatically — useful for chat boxes.",
        ),
        Line::from(
            "  • Post-process commands run on the transcript before output. \
             Edit the command body in config.toml; the cycler offers a few \
             starter presets.",
        ),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn yesno(b: bool) -> String {
    (if b { "yes" } else { "no" }).to_string()
}

fn display_append(s: Option<&str>) -> String {
    match s {
        None => "(none)".to_string(),
        Some(" ") => "space".to_string(),
        Some("\n") => "newline".to_string(),
        Some(other) => format!("{:?}", other),
    }
}

impl From<FeedbackLevel> for super::common::FeedbackLevel {
    fn from(v: FeedbackLevel) -> Self {
        match v {
            FeedbackLevel::Ok => super::common::FeedbackLevel::Ok,
            FeedbackLevel::Err => super::common::FeedbackLevel::Err,
        }
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.output.as_mut() {
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
