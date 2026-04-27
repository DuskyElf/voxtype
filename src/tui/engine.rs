//! Engine section: tunables for the active transcription engine.
//!
//! Currently focuses on Whisper, the default engine. Non-Whisper engines
//! show a placeholder pointing the user at config.toml until each one gets
//! its own form.

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
pub struct EngineState {
    pub engine: String,
    pub whisper: WhisperFields,
    pub field: WField,
    pub feedback: Option<Feedback>,
    pub dirty_since_load: bool,
}

#[derive(Debug, Clone)]
pub struct WhisperFields {
    pub mode: String, // local / remote / cli
    pub language: String,
    pub translate: bool,
    pub threads: Option<i64>,
    pub initial_prompt: Option<String>,
    pub flash_attention: bool,
    pub on_demand_loading: bool,
    pub gpu_isolation: bool,
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
pub enum WField {
    Mode,
    Language,
    Translate,
    Threads,
    Prompt,
    FlashAttention,
    OnDemandLoading,
    GpuIsolation,
}

impl WField {
    const ALL: &'static [WField] = &[
        WField::Mode,
        WField::Language,
        WField::Translate,
        WField::Threads,
        WField::Prompt,
        WField::FlashAttention,
        WField::OnDemandLoading,
        WField::GpuIsolation,
    ];
}

const MODE_CHOICES: &[&str] = &["local", "remote", "cli"];
const LANG_CHOICES: &[&str] = &[
    "auto", "en", "fr", "de", "it", "es", "pt", "nl", "pl", "zh", "ja", "ko", "ru", "ar",
];

impl EngineState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        let engine = ed
            .get_string("", "engine")
            .unwrap_or_else(|| "whisper".to_string());
        let whisper = WhisperFields {
            mode: ed
                .get_string("whisper", "mode")
                .unwrap_or_else(|| "local".to_string()),
            language: ed
                .get_string("whisper", "language")
                .unwrap_or_else(|| "auto".to_string()),
            translate: ed.get_bool("whisper", "translate").unwrap_or(false),
            threads: ed.get_int("whisper", "threads"),
            initial_prompt: ed.get_string("whisper", "initial_prompt"),
            flash_attention: ed.get_bool("whisper", "flash_attention").unwrap_or(false),
            on_demand_loading: ed.get_bool("whisper", "on_demand_loading").unwrap_or(false),
            gpu_isolation: ed.get_bool("whisper", "gpu_isolation").unwrap_or(false),
        };
        Ok(Self {
            engine,
            whisper,
            field: WField::Mode,
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

        let w = &self.whisper;
        ed.set_string("whisper", "mode", &w.mode);
        ed.set_string("whisper", "language", &w.language);
        ed.set_bool("whisper", "translate", w.translate);
        match w.threads {
            Some(n) => ed.set_int("whisper", "threads", n),
            None => ed.unset("whisper", "threads"),
        }
        match &w.initial_prompt {
            Some(p) if !p.is_empty() => ed.set_string("whisper", "initial_prompt", p),
            _ => ed.unset("whisper", "initial_prompt"),
        }
        ed.set_bool("whisper", "flash_attention", w.flash_attention);
        ed.set_bool("whisper", "on_demand_loading", w.on_demand_loading);
        ed.set_bool("whisper", "gpu_isolation", w.gpu_isolation);

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
        let len = WField::ALL.len() as i32;
        let cur = WField::ALL.iter().position(|f| *f == self.field).unwrap_or(0) as i32;
        let new = (cur + delta).rem_euclid(len);
        self.field = WField::ALL[new as usize];
    }

    fn cycle(&mut self, delta: i32) {
        let w = &mut self.whisper;
        match self.field {
            WField::Mode => {
                let idx = MODE_CHOICES
                    .iter()
                    .position(|c| *c == w.mode)
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(MODE_CHOICES.len() as i32);
                w.mode = MODE_CHOICES[n as usize].to_string();
            }
            WField::Language => {
                let idx = LANG_CHOICES
                    .iter()
                    .position(|c| *c == w.language)
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let n = (idx + delta).rem_euclid(LANG_CHOICES.len() as i32);
                w.language = LANG_CHOICES[n as usize].to_string();
            }
            WField::Translate => w.translate = !w.translate,
            WField::Threads => {
                let cur = w.threads.unwrap_or(0);
                let next = cur + delta as i64;
                w.threads = if next <= 0 { None } else { Some(next.min(64)) };
            }
            WField::Prompt => {
                // Toggle between (none) and a sample prompt; in-line editing
                // arrives in a later PR.
                if w.initial_prompt.is_some() {
                    w.initial_prompt = None;
                } else {
                    w.initial_prompt = Some(
                        "Transcribe with proper capitalization and punctuation.".to_string(),
                    );
                }
            }
            WField::FlashAttention => w.flash_attention = !w.flash_attention,
            WField::OnDemandLoading => w.on_demand_loading = !w.on_demand_loading,
            WField::GpuIsolation => w.gpu_isolation = !w.gpu_isolation,
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Engine");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let state = match &app.engine {
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

    if state.engine != "whisper" {
        let lines = vec![
            Line::from(Span::styled(
                format!("Active engine: {}", state.engine),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(format!(
                "Per-engine tuning for {} is not yet exposed in the TUI.",
                state.engine
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Edit ~/.config/voxtype/config.toml directly for now.",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if state.feedback.is_some() { 2 } else { 0 }),
            Constraint::Length(3), // header
            Constraint::Length(10), // form
            Constraint::Min(0),    // help
            Constraint::Length(1), // hint
        ])
        .split(inner);

    if let Some(fb) = &state.feedback {
        render_feedback(f, chunks[0], fb);
    }
    render_header(f, chunks[1], state);
    render_form(f, chunks[2], state);
    render_help(f, chunks[3]);
    render_hint(f, chunks[4], state);
}

fn render_feedback(f: &mut Frame, area: Rect, fb: &Feedback) {
    let style = match fb.level {
        FeedbackLevel::Ok => Style::default().fg(Color::Green),
        FeedbackLevel::Err => Style::default().fg(Color::Red),
    };
    let prefix = match fb.level {
        FeedbackLevel::Ok => "✓ ",
        FeedbackLevel::Err => "✗ ",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{}{}", prefix, fb.message),
            style,
        ))),
        area,
    );
}

fn render_header(f: &mut Frame, area: Rect, state: &EngineState) {
    let dirty = if state.dirty_since_load {
        Span::styled("  • unsaved", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Whisper",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            dirty,
        ]),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn render_form(f: &mut Frame, area: Rect, state: &EngineState) {
    let w = &state.whisper;
    let rows = [
        (WField::Mode, "Execution mode", w.mode.clone()),
        (WField::Language, "Language", w.language.clone()),
        (WField::Translate, "Translate to English", yesno(w.translate)),
        (
            WField::Threads,
            "Threads",
            w.threads
                .map(|n| n.to_string())
                .unwrap_or_else(|| "auto".to_string()),
        ),
        (
            WField::Prompt,
            "Initial prompt",
            w.initial_prompt
                .clone()
                .map(|s| {
                    if s.len() > 40 {
                        format!("{}…", &s[..40])
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| "(none)".to_string()),
        ),
        (
            WField::FlashAttention,
            "Flash attention",
            yesno(w.flash_attention),
        ),
        (
            WField::OnDemandLoading,
            "On-demand model loading",
            yesno(w.on_demand_loading),
        ),
        (
            WField::GpuIsolation,
            "GPU isolation (subprocess)",
            yesno(w.gpu_isolation),
        ),
    ];

    let lines: Vec<Line> = rows
        .iter()
        .map(|(field, label, value)| {
            let focused = *field == state.field;
            let label_style = if focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let value_style = if focused {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if focused { "▸ " } else { "  " };
            Line::from(vec![
                Span::styled(format!("{}{:<28}", prefix, label), label_style),
                Span::styled(format!(" ◂ {} ▸", value), value_style),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn yesno(b: bool) -> String {
    (if b { "yes" } else { "no" }).to_string()
}

fn render_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  • mode = remote: transcribe via an OpenAI-compatible HTTP API. \
             Configure remote_endpoint / remote_api_key separately.",
        ),
        Line::from(
            "  • Initial prompt cycles between (none) and a sample. Inline text \
             editing of the prompt arrives in a future release; for now edit \
             the prompt in config.toml directly.",
        ),
        Line::from(
            "  • GPU isolation runs each transcription in a subprocess that \
             exits afterward. Eliminates VRAM creep at the cost of model \
             load time on every recording.",
        ),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn render_hint(f: &mut Frame, area: Rect, state: &EngineState) {
    let dirty_marker = if state.dirty_since_load {
        Span::styled("  ●", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![
        Span::styled(
            " ↑↓ field   ←→ change   s save   r revert ",
            Style::default().fg(Color::DarkGray),
        ),
        dirty_marker,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.engine.as_mut() {
        Some(s) => s,
        None => return Action::None,
    };
    if state.engine != "whisper" {
        return Action::None;
    }
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
