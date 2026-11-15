use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, status_line, NodeStatus, MAX_VISUAL_NODES};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct MapNode {
    pub id: String,
    pub label: String,
    pub status: NodeStatus,
}

#[derive(Debug, Clone)]
pub struct MapLink {
    pub a: String,
    pub b: String,
    pub state: LinkState,
}

/// Nodes in a row joined by links; a down link shows a break marker. Falls back to a list
/// when the row does not fit or there are more than `MAX_VISUAL_NODES` nodes.
pub struct MapWidget {
    pub nodes: Vec<MapNode>,
    pub links: Vec<MapLink>,
    pub theme: Theme,
}

fn link_between<'a>(links: &'a [MapLink], a: &str, b: &str) -> Option<&'a MapLink> {
    links
        .iter()
        .find(|l| (l.a == a && l.b == b) || (l.a == b && l.b == a))
}

fn state_word(s: LinkState) -> &'static str {
    match s {
        LinkState::Up => "up",
        LinkState::Down => "down",
        LinkState::Unknown => "unknown",
    }
}

impl MapWidget {
    fn link_span(&self, state: Option<LinkState>) -> Span<'static> {
        match state {
            Some(LinkState::Up) => Span::styled("═══", Style::default().fg(self.theme.ok)),
            Some(LinkState::Down) => Span::styled("╌✗╌", Style::default().fg(self.theme.fail)),
            _ => Span::styled("···", Style::default().fg(self.theme.pending)),
        }
    }

    fn render_list(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = self
            .nodes
            .iter()
            .map(|n| status_line(n.status, &n.label, &self.theme))
            .collect();
        for l in &self.links {
            lines.push(Line::from(format!(
                "  {} ─ {}: {}",
                l.a,
                l.b,
                state_word(l.state)
            )));
        }
        draw_lines(lines, area, buf);
    }
}

impl Widget for MapWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let boxes: Vec<usize> = self
            .nodes
            .iter()
            .map(|n| n.label.chars().count() + 4)
            .collect();
        let needed: usize = boxes.iter().sum::<usize>() + 3 * self.nodes.len().saturating_sub(1);
        if self.nodes.len() > MAX_VISUAL_NODES || needed > area.width as usize {
            self.render_list(area, buf);
            return;
        }
        let mut spans: Vec<Span> = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if i > 0 {
                let prev = &self.nodes[i - 1];
                let st = link_between(&self.links, &prev.id, &n.id).map(|l| l.state);
                spans.push(self.link_span(st));
            }
            spans.push(Span::raw("["));
            spans.push(Span::styled(n.status.glyph(), n.status.style(&self.theme)));
            spans.push(Span::raw(format!(" {}]", n.label)));
        }
        let mut lines = vec![Line::from(spans)];
        // links between nodes that are not neighbours in the row
        for l in &self.links {
            let ia = self.nodes.iter().position(|n| n.id == l.a);
            let ib = self.nodes.iter().position(|n| n.id == l.b);
            if let (Some(a), Some(b)) = (ia, ib) {
                if a.abs_diff(b) != 1 {
                    lines.push(Line::from(format!(
                        "  {} ─ {}: {}",
                        l.a,
                        l.b,
                        state_word(l.state)
                    )));
                }
            }
        }
        draw_lines(lines, area, buf);
    }
}
