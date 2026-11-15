use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, NodeStatus};
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct PipelineLink {
    pub label: String,
    pub status: NodeStatus,
    pub detail: Option<String>,
}

/// A vertical chain of links joined by connectors. Each link shows a status glyph (a spinner
/// while it is running), its label and an optional dim detail.
pub struct PipelineWidget {
    pub links: Vec<PipelineLink>,
    pub frame: u64,
    pub theme: Theme,
}

impl Widget for PipelineWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = self
            .links
            .iter()
            .map(|l| l.label.chars().count())
            .max()
            .unwrap_or(0);
        let mut lines: Vec<Line> = Vec::new();
        for (i, l) in self.links.iter().enumerate() {
            if i > 0 {
                lines.push(Line::styled("│", Style::default().fg(self.theme.pending)));
            }
            let mut spans = vec![
                Span::styled(l.status.glyph_at(self.frame), l.status.style(&self.theme)),
                Span::raw(format!(" {:<width$}", l.label)),
            ];
            if let Some(d) = &l.detail {
                spans.push(Span::styled(
                    format!("  {d}"),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));
        }
        draw_lines(lines, area, buf);
    }
}
