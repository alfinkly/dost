use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, status_line, NodeStatus};
use crate::theme::Theme;

const SEP: &str = " ─▶ ";

/// Steps in order with a status glyph each; falls back to one step per line when narrow.
pub struct ChainWidget {
    pub steps: Vec<(String, NodeStatus)>,
    pub theme: Theme,
}

impl Widget for ChainWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let needed: usize = self
            .steps
            .iter()
            .map(|(n, _)| n.chars().count() + 2)
            .sum::<usize>()
            + SEP.chars().count() * self.steps.len().saturating_sub(1);
        if needed <= area.width as usize {
            let mut spans = Vec::new();
            for (i, (name, st)) in self.steps.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(SEP));
                }
                spans.push(Span::styled(st.glyph(), st.style(&self.theme)));
                spans.push(Span::raw(format!(" {name}")));
            }
            draw_lines(vec![Line::from(spans)], area, buf);
        } else {
            let lines = self
                .steps
                .iter()
                .map(|(n, st)| status_line(*st, n, &self.theme))
                .collect();
            draw_lines(lines, area, buf);
        }
    }
}
