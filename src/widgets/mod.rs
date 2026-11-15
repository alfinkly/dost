//! Level-1 visual blocks: chain, table, map, graph and progress.

mod chain;
mod graph;
mod map;
mod progress;
mod table;
mod tree;

pub use chain::ChainWidget;
pub use graph::GraphWidget;
pub use map::{LinkState, MapLink, MapNode, MapWidget};
pub use progress::ProgressWidget;
pub use table::{Cell, TableWidget};
pub use tree::{TreeNode, TreeWidget};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// More nodes than this are always drawn as a plain list.
pub const MAX_VISUAL_NODES: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Pending,
    Running,
    Ok,
    Failed,
    /// Nothing is known about this node in this run.
    Unknown,
}

impl NodeStatus {
    pub fn glyph(self) -> &'static str {
        match self {
            NodeStatus::Pending => "○",
            NodeStatus::Running => "⠹",
            NodeStatus::Ok => "✓",
            NodeStatus::Failed => "✗",
            NodeStatus::Unknown => "·",
        }
    }

    /// Like `glyph`, but a running node shows a spinner frame.
    pub fn glyph_at(self, frame: u64) -> &'static str {
        const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        match self {
            NodeStatus::Running => SPINNER[(frame % 10) as usize],
            other => other.glyph(),
        }
    }

    pub fn style(self, theme: &Theme) -> Style {
        let c = match self {
            NodeStatus::Pending | NodeStatus::Unknown => theme.pending,
            NodeStatus::Running => theme.running,
            NodeStatus::Ok => theme.ok,
            NodeStatus::Failed => theme.fail,
        };
        let s = Style::default().fg(c);
        if self == NodeStatus::Failed {
            s.add_modifier(Modifier::BOLD)
        } else {
            s
        }
    }
}

/// Draw `lines` one per row, clipped to `area`.
pub(crate) fn draw_lines(lines: Vec<Line<'_>>, area: Rect, buf: &mut Buffer) {
    for (i, line) in lines.iter().enumerate() {
        if i as u16 >= area.height {
            break;
        }
        buf.set_line(area.x, area.y + i as u16, line, area.width);
    }
}

pub(crate) fn status_line<'a>(status: NodeStatus, label: &'a str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(status.glyph(), status.style(theme)),
        Span::raw(" "),
        Span::raw(label),
    ])
}
