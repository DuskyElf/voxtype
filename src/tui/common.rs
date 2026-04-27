//! Shared rendering helpers for form-style sections (Hotkey, Audio, Output, …).

#![allow(dead_code)]

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, Clone, Copy)]
pub enum FeedbackLevel {
    Ok,
    Err,
}

pub fn render_feedback(f: &mut Frame, area: Rect, level: FeedbackLevel, message: &str) {
    let style = match level {
        FeedbackLevel::Ok => Style::default().fg(Color::Green),
        FeedbackLevel::Err => Style::default().fg(Color::Red),
    };
    let prefix = match level {
        FeedbackLevel::Ok => "✓ ",
        FeedbackLevel::Err => "✗ ",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{}{}", prefix, message),
            style,
        ))),
        area,
    );
}

pub fn render_section_header(f: &mut Frame, area: Rect, title: &str, dirty: bool) {
    let dirty_span = if dirty {
        Span::styled("  • unsaved", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };
    let line = Line::from(vec![
        Span::styled(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        dirty_span,
    ]);
    f.render_widget(Paragraph::new(vec![line, Line::from("")]), area);
}

pub fn render_bottom_hint(f: &mut Frame, area: Rect, dirty: bool) {
    let dirty_marker = if dirty {
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

/// Single form row: focused or unfocused, with a label-and-value layout that
/// matches the rest of the form sections.
pub fn form_row<'a>(focused: bool, label: &str, value: &str) -> Line<'a> {
    form_row_dimmed(focused, false, label, value)
}

/// Form row that supports a `dimmed` variant for fields disabled by another
/// toggle (e.g. the rest of the Hotkey form when the evdev listener is off).
pub fn form_row_dimmed<'a>(
    focused: bool,
    dimmed: bool,
    label: &str,
    value: &str,
) -> Line<'a> {
    let dim_color = Color::DarkGray;
    let label_style = if dimmed {
        Style::default().fg(dim_color)
    } else if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let value_style = if dimmed {
        Style::default().fg(dim_color)
    } else if focused {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    let prefix = if focused { "▸ " } else { "  " };
    Line::from(vec![
        Span::styled(format!("{}{:<32}", prefix, label), label_style),
        Span::styled(format!(" ◂ {} ▸", value), value_style),
    ])
}

/// Specification for a row in a two-pane form.
pub struct FormRowSpec {
    pub focused: bool,
    pub dimmed: bool,
    pub label: String,
    pub value: String,
}

impl FormRowSpec {
    pub fn new(focused: bool, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            focused,
            dimmed: false,
            label: label.into(),
            value: value.into(),
        }
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }
}

/// Render a section using the General-style two-panel layout: a form panel on
/// the left (rows, save/revert hints) and a guidance panel on the right that
/// shows context-sensitive help for the focused row.
///
/// Layout (vertical):
///   1 row  feedback (only present if `feedback` is Some)
///   2 rows section title + dirty marker
///   N rows two columns: form (Settings) on left, guidance (About) on right
///   1 row  bottom hint
pub fn render_form_with_guidance(
    f: &mut Frame,
    area: Rect,
    title: &str,
    dirty: bool,
    feedback: Option<(FeedbackLevel, &str)>,
    rows: &[FormRowSpec],
    guidance: Vec<Line<'_>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if feedback.is_some() { 2 } else { 0 }),
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area);

    if let Some((lvl, msg)) = feedback {
        render_feedback(f, chunks[0], lvl, msg);
    }
    render_section_header(f, chunks[1], title, dirty);

    // Two columns: Settings on the left, About on the right.
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[2]);

    render_settings_panel(f, body[0], rows);
    render_guidance_panel(f, body[1], guidance);

    render_bottom_hint(f, chunks[3], dirty);
}

fn render_settings_panel(f: &mut Frame, area: Rect, rows: &[FormRowSpec]) {
    let block = Block::default().borders(Borders::ALL).title("Settings");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = rows
        .iter()
        .map(|r| form_row_dimmed(r.focused, r.dimmed, &r.label, &r.value))
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_guidance_panel(f: &mut Frame, area: Rect, lines: Vec<Line<'_>>) {
    let block = Block::default().borders(Borders::ALL).title("About");
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
