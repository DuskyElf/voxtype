//! Models section: current engine + model picker for the active engine.
//!
//! Lists models known to voxtype for the configured engine, marks which are
//! installed on disk (under `~/.local/share/voxtype/models/`), and lets the
//! user pick one. Downloads still happen via `voxtype setup model`; this
//! section is for switching between models you've already pulled.

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
use crate::config::Config;
use crate::setup::model;

#[derive(Debug, Clone)]
pub struct ModelsState {
    pub engine: String,
    pub current_model: String,
    /// (model name, installed?) tuples for the active engine.
    pub catalog: Vec<(String, bool)>,
    pub cursor: usize,
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

impl ModelsState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        let engine = ed
            .get_string("", "engine")
            .or_else(|| ed.get_string("transcription", "engine"))
            .unwrap_or_else(|| "whisper".to_string());

        let current_model = match engine.as_str() {
            "whisper" => ed
                .get_string("whisper", "model")
                .unwrap_or_else(|| "small.en".to_string()),
            "parakeet" => ed
                .get_string("parakeet", "model")
                .unwrap_or_else(|| "parakeet-tdt-0.6b-v3".to_string()),
            other => ed
                .get_string(other, "model")
                .unwrap_or_else(|| String::new()),
        };

        let catalog = build_catalog(&engine);
        let cursor = catalog
            .iter()
            .position(|(n, _)| *n == current_model)
            .unwrap_or(0);

        Ok(Self {
            engine,
            current_model,
            catalog,
            cursor,
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
        let table = match self.engine.as_str() {
            "parakeet" => "parakeet",
            "moonshine" => "moonshine",
            "sensevoice" => "sensevoice",
            "paraformer" => "paraformer",
            "dolphin" => "dolphin",
            "omnilingual" => "omnilingual",
            _ => "whisper",
        };
        ed.set_string(table, "model", &self.current_model);

        match ed.save() {
            Ok(()) => {
                self.dirty_since_load = false;
                self.feedback = Some(Feedback {
                    level: FeedbackLevel::Ok,
                    message: format!(
                        "Saved [{}] model = {}",
                        table, self.current_model
                    ),
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
                let cursor = self.cursor;
                *self = fresh;
                self.cursor = cursor.min(self.catalog.len().saturating_sub(1));
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

    fn move_cursor(&mut self, delta: i32) {
        if self.catalog.is_empty() {
            return;
        }
        let len = self.catalog.len() as i32;
        let new = (self.cursor as i32 + delta).rem_euclid(len);
        self.cursor = new as usize;
    }

    fn select_at_cursor(&mut self) {
        if let Some((name, _)) = self.catalog.get(self.cursor) {
            if *name != self.current_model {
                self.current_model = name.clone();
                self.dirty_since_load = true;
                self.feedback = None;
            }
        }
    }
}

fn build_catalog(engine: &str) -> Vec<(String, bool)> {
    let names: Vec<&'static str> = match engine {
        "whisper" => model::valid_model_names(),
        "parakeet" => model::valid_parakeet_model_names(),
        "moonshine" => model::valid_moonshine_model_names(),
        "sensevoice" => model::valid_sensevoice_model_names(),
        _ => Vec::new(),
    };

    let models_dir = Config::models_dir();
    names
        .into_iter()
        .map(|n| {
            let installed = match engine {
                "whisper" => models_dir.join(whisper_filename(n)).exists(),
                _ => models_dir.join(n).exists() || models_dir.join(n).is_dir(),
            };
            (n.to_string(), installed)
        })
        .collect()
}

/// Mirror of setup::model::get_model_filename — match Whisper's "ggml-{name}.bin"
/// convention.
fn whisper_filename(name: &str) -> String {
    format!("ggml-{}.bin", name)
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Models");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let state = match &app.models {
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
            Constraint::Length(4), // header
            Constraint::Min(0),    // model list
            Constraint::Length(2), // help
            Constraint::Length(1), // hint
        ])
        .split(inner);

    if let Some(fb) = &state.feedback {
        render_feedback(f, chunks[0], fb);
    }
    render_header(f, chunks[1], state);
    render_list(f, chunks[2], state);
    render_help_text(f, chunks[3]);
    render_bottom_hint(f, chunks[4], state);
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

fn render_header(f: &mut Frame, area: Rect, state: &ModelsState) {
    let dirty = if state.dirty_since_load {
        Span::styled("  • unsaved", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Models",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            dirty,
        ]),
        Line::from(format!("Engine:    {}", state.engine)),
        Line::from(format!("Current:   {}", state.current_model)),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn render_list(f: &mut Frame, area: Rect, state: &ModelsState) {
    if state.catalog.is_empty() {
        let lines = vec![
            Line::from(""),
            Line::from(format!(
                "No models known for engine '{}'.",
                state.engine
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Run `voxtype setup model` to download.",
                Style::default().fg(Color::Gray),
            )),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
        return;
    }

    let lines: Vec<Line> = state
        .catalog
        .iter()
        .enumerate()
        .map(|(i, (name, installed))| {
            let focused = i == state.cursor;
            let active = *name == state.current_model;

            let marker = if active {
                Span::styled("● ", Style::default().fg(Color::Green))
            } else if *installed {
                Span::styled("✓ ", Style::default().fg(Color::Cyan))
            } else {
                Span::styled("· ", Style::default().fg(Color::Gray))
            };

            let cursor_glyph = if focused { "▸ " } else { "  " };
            let row_style = if focused {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            let suffix = if *installed { "" } else { "  (not downloaded)" };

            Line::from(vec![
                Span::raw(cursor_glyph),
                marker,
                Span::styled(format!("{}{}", name, suffix), row_style),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines), area);
}

fn render_help_text(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "  ● = active   ✓ = installed   · = not downloaded yet (run \
             `voxtype setup model` to fetch)",
        ),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn render_bottom_hint(f: &mut Frame, area: Rect, state: &ModelsState) {
    let dirty_marker = if state.dirty_since_load {
        Span::styled("  ●", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![
        Span::styled(
            " ↑↓ navigate   Enter select   s save   r revert ",
            Style::default().fg(Color::Gray),
        ),
        dirty_marker,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.models.as_mut() {
        Some(s) => s,
        None => return Action::None,
    };
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_cursor(-1);
            Action::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_cursor(1);
            Action::None
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            state.select_at_cursor();
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
