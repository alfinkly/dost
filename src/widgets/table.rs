use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::draw_lines;
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub changed: bool,
}

pub struct TableWidget {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub highlight_changed: bool,
    pub theme: Theme,
}

impl Widget for TableWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let n = self.columns.len();
        let mut widths: Vec<usize> = self.columns.iter().map(|c| c.chars().count()).collect();
        for row in &self.rows {
            for (i, c) in row.iter().enumerate().take(n) {
                widths[i] = widths[i].max(c.text.chars().count());
            }
        }
        let pad = |s: &str, w: usize| format!("{:<w$}  ", s, w = w);
        let mut lines = Vec::new();
        lines.push(Line::from(
            self.columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    Span::styled(
                        pad(c, widths[i]),
                        Style::default().add_modifier(Modifier::BOLD),
                    )
                })
                .collect::<Vec<_>>(),
        ));
        for row in &self.rows {
            let spans: Vec<Span> = row
                .iter()
                .enumerate()
                .take(n)
                .map(|(i, c)| {
                    let style = if self.highlight_changed && c.changed {
                        Style::default()
                            .fg(self.theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Span::styled(pad(&c.text, widths[i]), style)
                })
                .collect();
            lines.push(Line::from(spans));
        }
        draw_lines(lines, area, buf);
    }
}
