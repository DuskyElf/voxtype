//! Audio settings: input device, max duration, feedback sounds, MPRIS pause.

use cpal::traits::{DeviceTrait, HostTrait};
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
pub struct AudioState {
    pub device: String,
    pub max_duration_secs: u32,
    pub pause_media: bool,
    pub feedback_enabled: bool,
    pub feedback_theme: String,
    pub feedback_volume: f32,

    pub field: Field,
    pub feedback: Option<Feedback>,
    pub dirty_since_load: bool,
    /// Cached device list (default + everything cpal finds). Loaded once.
    pub device_choices: Vec<String>,
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
    Device,
    MaxDuration,
    PauseMedia,
    FeedbackEnabled,
    FeedbackTheme,
    FeedbackVolume,
}

impl Field {
    const ALL: &'static [Field] = &[
        Field::Device,
        Field::MaxDuration,
        Field::PauseMedia,
        Field::FeedbackEnabled,
        Field::FeedbackTheme,
        Field::FeedbackVolume,
    ];
}

const THEME_CHOICES: &[&str] = &["default", "subtle", "mechanical"];
/// Step in seconds for the max-duration cycler.
const DURATION_STEP: u32 = 30;
const DURATION_MIN: u32 = 30;
const DURATION_MAX: u32 = 1800;
const VOLUME_STEP: f32 = 0.1;

impl AudioState {
    pub fn load() -> Result<Self, EditorError> {
        let ed = ConfigEditor::load()?;
        Ok(Self {
            device: ed
                .get_string("audio", "device")
                .unwrap_or_else(|| "default".to_string()),
            max_duration_secs: ed
                .get_int("audio", "max_duration_secs")
                .map(|n| n.clamp(0, u32::MAX as i64) as u32)
                .unwrap_or(120),
            pause_media: ed.get_bool("audio", "pause_media").unwrap_or(false),
            feedback_enabled: ed
                .get_bool("audio.feedback", "enabled")
                .unwrap_or(false),
            feedback_theme: ed
                .get_string("audio.feedback", "theme")
                .unwrap_or_else(|| "default".to_string()),
            feedback_volume: ed
                .get_string("audio.feedback", "volume")
                .and_then(|s| s.parse().ok())
                .or_else(|| {
                    ed.get_int("audio.feedback", "volume")
                        .map(|n| n as f32)
                })
                .unwrap_or(0.7),
            field: Field::Device,
            feedback: None,
            dirty_since_load: false,
            device_choices: enumerate_input_devices(),
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
        ed.set_string("audio", "device", &self.device);
        ed.set_int(
            "audio",
            "max_duration_secs",
            self.max_duration_secs as i64,
        );
        ed.set_bool("audio", "pause_media", self.pause_media);
        ed.set_bool("audio.feedback", "enabled", self.feedback_enabled);
        ed.set_string("audio.feedback", "theme", &self.feedback_theme);
        ed.set_string(
            "audio.feedback",
            "volume",
            &format!("{:.2}", self.feedback_volume),
        );

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
                let cached = self.device_choices.clone();
                *self = fresh;
                self.field = field;
                self.device_choices = cached;
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
            Field::Device => {
                if !self.device_choices.is_empty() {
                    let idx = self
                        .device_choices
                        .iter()
                        .position(|d| d == &self.device)
                        .map(|i| i as i32)
                        .unwrap_or(-1);
                    let new = (idx + delta).rem_euclid(self.device_choices.len() as i32);
                    self.device = self.device_choices[new as usize].clone();
                }
            }
            Field::MaxDuration => {
                let next = self.max_duration_secs as i32 + delta * DURATION_STEP as i32;
                self.max_duration_secs =
                    next.clamp(DURATION_MIN as i32, DURATION_MAX as i32) as u32;
            }
            Field::PauseMedia => {
                self.pause_media = !self.pause_media;
            }
            Field::FeedbackEnabled => {
                self.feedback_enabled = !self.feedback_enabled;
            }
            Field::FeedbackTheme => {
                let idx = THEME_CHOICES
                    .iter()
                    .position(|t| *t == self.feedback_theme)
                    .map(|i| i as i32)
                    .unwrap_or(-1);
                let new = (idx + delta).rem_euclid(THEME_CHOICES.len() as i32);
                self.feedback_theme = THEME_CHOICES[new as usize].to_string();
            }
            Field::FeedbackVolume => {
                let next = self.feedback_volume + delta as f32 * VOLUME_STEP;
                self.feedback_volume = next.clamp(0.0, 1.0);
            }
        }
        self.dirty_since_load = true;
        self.feedback = None;
    }
}

fn enumerate_input_devices() -> Vec<String> {
    let mut out = vec!["default".to_string()];
    let host = cpal::default_host();
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                if name != "default" && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("Audio");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let state = match &app.audio {
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
            Constraint::Length(8), // form
            Constraint::Min(0),    // help
            Constraint::Length(1), // bottom hint
        ])
        .split(inner);

    if let Some(fb) = &state.feedback {
        render_feedback(f, chunks[0], fb);
    }
    render_header(f, chunks[1], state);
    render_form(f, chunks[2], state);
    render_help_text(f, chunks[3], state);
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

fn render_header(f: &mut Frame, area: Rect, state: &AudioState) {
    let dirty = if state.dirty_since_load {
        Span::styled("  • unsaved", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![
        Span::styled(
            "Audio",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        dirty,
    ]);
    f.render_widget(Paragraph::new(vec![line, Line::from("")]), area);
}

fn render_form(f: &mut Frame, area: Rect, state: &AudioState) {
    let rows = [
        (
            Field::Device,
            "Input device",
            display_device(&state.device, state.device_choices.len()),
        ),
        (
            Field::MaxDuration,
            "Max recording (seconds)",
            state.max_duration_secs.to_string(),
        ),
        (
            Field::PauseMedia,
            "Pause MPRIS media on record",
            (if state.pause_media { "yes" } else { "no" }).to_string(),
        ),
        (
            Field::FeedbackEnabled,
            "Audio feedback sounds",
            (if state.feedback_enabled { "on" } else { "off" }).to_string(),
        ),
        (
            Field::FeedbackTheme,
            "Sound theme",
            state.feedback_theme.clone(),
        ),
        (
            Field::FeedbackVolume,
            "Volume",
            format!("{:.0}%", state.feedback_volume * 100.0),
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
                Span::styled(format!("{}{:<32}", prefix, label), label_style),
                Span::styled(format!(" ◂ {} ▸", value), value_style),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines), area);
}

fn render_help_text(f: &mut Frame, area: Rect, state: &AudioState) {
    let device_count = state.device_choices.len().saturating_sub(1);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "  • Detected {} input device{} via cpal. \"default\" follows your \
             PipeWire/PulseAudio default source.",
            device_count,
            if device_count == 1 { "" } else { "s" },
        )),
        Line::from(""),
        Line::from(
            "  • Max recording is a safety cap. Voxtype stops recording at this length \
             and transcribes whatever it captured.",
        ),
        Line::from(""),
        Line::from(
            "  • Pause-MPRIS uses playerctl to pause Spotify/MPV/etc. while you dictate \
             and resume them on stop. Requires playerctl to be installed.",
        ),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn render_bottom_hint(f: &mut Frame, area: Rect, state: &AudioState) {
    let dirty_marker = if state.dirty_since_load {
        Span::styled("  ●", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![
        Span::styled(
            " ↑↓ field   ←→ change   s save   r revert ",
            Style::default().fg(Color::Gray),
        ),
        dirty_marker,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn display_device(device: &str, total: usize) -> String {
    if total <= 1 {
        device.to_string()
    } else {
        format!("{}", device)
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    let state = match app.audio.as_mut() {
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
