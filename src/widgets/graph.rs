use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, MapNode};
use crate::theme::Theme;

/// Dependency graph shown as one line per node: `status label ◂ depends, on`.
/// An edge `(from, to)` means `from` depends on `to`.
pub struct GraphWidget {
    pub nodes: Vec<MapNode>,
    pub edges: Vec<(String, String)>,
    pub theme: Theme,
}

impl Widget for GraphWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line> = self
            .nodes
            .iter()
            .map(|n| {
                let deps: Vec<&str> = self
                    .edges
                    .iter()
                    .filter(|(from, _)| *from == n.id)
                    .map(|(_, to)| to.as_str())
                    .collect();
                let mut spans = vec![
                    Span::styled(n.status.glyph(), n.status.style(&self.theme)),
                    Span::raw(format!(" {}", n.label)),
                ];
                if !deps.is_empty() {
                    spans.push(Span::raw(format!(" ◂ {}", deps.join(", "))));
                }
                Line::from(spans)
            })
            .collect();
        draw_lines(lines, area, buf);
    }
}
