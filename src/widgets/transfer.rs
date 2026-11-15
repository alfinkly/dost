use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, NodeStatus};
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct TransferRow {
    pub label: String,
    pub from: String,
    pub to: String,
    pub status: NodeStatus,
    /// Bytes moved and the maximum, when known: the row then shows a bar and `done / total`.
    pub done: Option<u64>,
    pub total: Option<u64>,
    pub detail: Option<String>,
    /// Frames since the row first appeared; a freshly arrived row animates a packet crossing over.
    pub age: u64,
}

/// Rows of `from ──▶ to`: a packet crosses a track while a transfer runs, a bar fills when the
/// total is known, a settled row is a solid arrow and a failed one a broken arrow.
pub struct TransferWidget {
    pub titles: (String, String),
    pub rows: Vec<TransferRow>,
    pub frame: u64,
    pub theme: Theme,
}

const TRACK: usize = 14;
const ARRIVAL_FRAMES: u64 = 8;

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

impl TransferWidget {
    fn track(&self, r: &TransferRow) -> (String, Option<String>) {
        let packet = |pos: usize| {
            let mut s: Vec<char> = "─".repeat(TRACK).chars().collect();
            s[pos.min(TRACK - 1)] = '•';
            format!("{}▶", s.into_iter().collect::<String>())
        };
        match r.status {
            NodeStatus::Failed => (format!("{}╳", "─".repeat(TRACK)), None),
            NodeStatus::Pending | NodeStatus::Unknown => (format!("{}▶", "·".repeat(TRACK)), None),
            NodeStatus::Running => match (r.done, r.total) {
                (Some(d), Some(t)) if t > 0 => {
                    let filled = ((d.min(t) as u128 * TRACK as u128) / t as u128) as usize;
                    (
                        format!("{}{}", "█".repeat(filled), "░".repeat(TRACK - filled)),
                        Some(format!("{} / {}", human_bytes(d), human_bytes(t))),
                    )
                }
                _ => (packet((self.frame as usize) % TRACK), None),
            },
            NodeStatus::Ok => {
                if r.age < ARRIVAL_FRAMES {
                    let pos = (r.age as usize * TRACK) / ARRIVAL_FRAMES as usize;
                    (packet(pos), None)
                } else {
                    let note = match (r.done, r.total) {
                        (_, Some(t)) if t > 0 => {
                            Some(format!("{} / {}", human_bytes(t), human_bytes(t)))
                        }
                        _ => None,
                    };
                    (format!("{}▶", "═".repeat(TRACK)), note)
                }
            }
        }
    }
}

impl Widget for TransferWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = |f: fn(&TransferRow) -> usize, t: usize| {
            self.rows.iter().map(f).max().unwrap_or(0).max(t)
        };
        let lw = w(|r| r.label.chars().count(), 0);
        let fw = w(|r| r.from.chars().count(), self.titles.0.chars().count());
        let tw = w(|r| r.to.chars().count(), self.titles.1.chars().count());
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let dim = Style::default().add_modifier(Modifier::DIM);
        let mut lines: Vec<Line> = vec![Line::from(vec![
            Span::raw(format!("  {:<lw$}  ", "")),
            Span::styled(format!("{:<fw$}", self.titles.0), bold),
            Span::raw(format!("  {:<width$}  ", "", width = TRACK + 1)),
            Span::styled(self.titles.1.clone(), bold),
        ])];
        for r in &self.rows {
            let (track, note) = self.track(r);
            let color = match r.status {
                NodeStatus::Ok => self.theme.ok,
                NodeStatus::Failed => self.theme.fail,
                NodeStatus::Running => self.theme.running,
                _ => self.theme.pending,
            };
            let mut spans = vec![
                Span::styled(r.status.glyph_at(self.frame), r.status.style(&self.theme)),
                Span::raw(format!(" {:<lw$}  ", r.label)),
                Span::raw(format!("{:<fw$}", r.from)),
                Span::styled(format!("  {track}  "), Style::default().fg(color)),
                Span::raw(format!("{:<tw$}", r.to)),
            ];
            let tail = r.detail.clone().or(note);
            if let Some(t) = tail {
                spans.push(Span::styled(format!("  {t}"), dim));
            }
            lines.push(Line::from(spans));
        }
        draw_lines(lines, area, buf);
    }
}
