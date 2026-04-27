//! Shared rendering helpers for form-style sections (Hotkey, Audio, Output, …).

#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
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
            Style::default().fg(Color::DarkGray),
        ),
        dirty_marker,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// Single form row: focused or unfocused, with a label-and-value layout that
/// matches the rest of the form sections.
pub fn form_row<'a>(focused: bool, label: &str, value: &str) -> Line<'a> {
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
}
