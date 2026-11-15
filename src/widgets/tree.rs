use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{draw_lines, NodeStatus};
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
    pub status: NodeStatus,
}

/// A hierarchy drawn top-down with connectors; a running node shows a spinner frame.
/// A node whose parent is unknown is shown as a root. Long lines are clipped.
pub struct TreeWidget {
    pub nodes: Vec<TreeNode>,
    pub frame: u64,
    pub theme: Theme,
}

impl TreeWidget {
    fn walk(
        &self,
        idx: usize,
        prefix: &str,
        last: bool,
        root: bool,
        seen: &mut Vec<bool>,
        lines: &mut Vec<Line<'static>>,
    ) {
        if seen[idx] {
            return;
        }
        seen[idx] = true;
        let n = &self.nodes[idx];
        let branch = if root {
            ""
        } else if last {
            "└─ "
        } else {
            "├─ "
        };
        lines.push(Line::from(vec![
            Span::raw(format!("{prefix}{branch}")),
            Span::styled(n.status.glyph_at(self.frame), n.status.style(&self.theme)),
            Span::styled(format!(" {}", n.label), Style::default()),
        ]));
        let child_prefix = if root {
            String::new()
        } else {
            format!("{prefix}{}", if last { "   " } else { "│  " })
        };
        let children: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].parent.as_deref() == Some(n.id.as_str()))
            .collect();
        for (k, c) in children.iter().enumerate() {
            self.walk(
                *c,
                &child_prefix,
                k + 1 == children.len(),
                false,
                seen,
                lines,
            );
        }
    }
}

impl Widget for TreeWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let ids: Vec<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut seen = vec![false; self.nodes.len()];
        let mut lines = Vec::new();
        for i in 0..self.nodes.len() {
            let is_root = match &self.nodes[i].parent {
                None => true,
                Some(p) => !ids.contains(&p.as_str()),
            };
            if is_root {
                self.walk(i, "", true, true, &mut seen, &mut lines);
            }
        }
        draw_lines(lines, area, buf);
    }
}
