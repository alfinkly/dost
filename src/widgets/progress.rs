use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::draw_lines;
use crate::theme::Theme;

/// `label ██████░░░░ 3/6`
pub struct ProgressWidget {
    pub step: u32,
    pub of: u32,
    pub label: String,
    pub theme: Theme,
}

impl Widget for ProgressWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let ratio = format!("{}/{}", self.step, self.of);
        let used = self.label.chars().count() + ratio.chars().count() + 2;
        let bar_w = (area.width as usize).saturating_sub(used).clamp(0, 40);
        let filled = if self.of == 0 {
            0
        } else {
            bar_w * self.step.min(self.of) as usize / self.of as usize
        };
        let line = Line::from(vec![
            Span::raw(format!("{} ", self.label)),
            Span::styled("█".repeat(filled), Style::default().fg(self.theme.ok)),
            Span::styled(
                "░".repeat(bar_w - filled),
                Style::default().fg(self.theme.pending),
            ),
            Span::raw(format!(" {ratio}")),
        ]);
        draw_lines(vec![line], area, buf);
    }
}
