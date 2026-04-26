//! Placeholder screen for sections that haven't been built yet.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::section::Section;

pub fn render(f: &mut Frame, area: Rect, section: Section) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(section.label());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(Span::styled(
            section.label(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            section.summary(),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from("Coming soon."),
        Line::from(""),
        Line::from(Span::styled(
            "This section is part of the configuration TUI rollout. \
             The General section is functional today; remaining sections \
             ship in subsequent releases. Until then, edit the corresponding \
             fields in ~/.config/voxtype/config.toml directly.",
            Style::default().fg(Color::Gray),
        )),
    ];

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}
