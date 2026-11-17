//! `matrix` scene: the things a loop walks over (people, hosts, buckets) are the rows, the steps of
//! the run are the columns, every cell says what happened to that row in that step.
//!
//! Like `report` it needs no events: it reads the loop-item results of the run.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use serde::Deserialize;

use super::{row_state, task_matches, RowState, Scene, SceneDef, SceneError, StageDef};
use crate::stream::{RunMode, RunState};

#[derive(Debug, Clone, Deserialize)]
struct ColumnDef {
    label: String,
    task: String,
    /// Said under the table for every row that has no result in this column although the
    /// column's task has run (a person without an SSH key, for example).
    #[serde(default)]
    missing: Option<String>,
}

#[derive(Debug, Clone)]
struct Column {
    label: String,
    task: Regex,
    missing: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Matrix {
    pub(super) stages: Vec<(Regex, String)>,
    columns: Vec<Column>,
}

pub(super) fn parse_matrix(def: &SceneDef) -> Result<Matrix, SceneError> {
    let err = |reason: String| SceneError::Matrix {
        id: def.id.clone(),
        reason,
    };
    let re = |p: &str| Regex::new(p).map_err(|e| err(format!("invalid regex `{p}`: {e}")));
    let raw: Vec<ColumnDef> = serde_json::from_value(
        def.bind
            .get("columns")
            .cloned()
            .ok_or_else(|| err("needs bind.columns".into()))?,
    )
    .map_err(|e| err(format!("bind.columns: {e}")))?;
    if raw.is_empty() {
        return Err(err("bind.columns is empty".into()));
    }
    let raw_stages: Vec<StageDef> = match def.bind.get("stages") {
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|e| err(format!("bind.stages: {e}")))?
        }
        None => Vec::new(),
    };
    let mut stages = Vec::new();
    for s in raw_stages {
        stages.push((re(&s.play)?, s.label));
    }
    let mut columns = Vec::new();
    for c in raw {
        columns.push(Column {
            task: re(&c.task)?,
            label: c.label,
            missing: c.missing,
        });
    }
    Ok(Matrix { stages, columns })
}

fn rank(s: RowState) -> u8 {
    match s {
        RowState::Failed => 3,
        RowState::Changed => 2,
        RowState::Ok => 1,
        RowState::Skipped => 0,
    }
}

impl Matrix {
    /// Row names in the order they first appear, looking at the columns left to right.
    fn row_names(&self, state: &RunState) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        for col in &self.columns {
            for i in state
                .items
                .iter()
                .filter(|i| task_matches(&col.task, &i.task))
            {
                if !i.item.trim().is_empty() && !rows.contains(&i.item) {
                    rows.push(i.item.clone());
                }
            }
        }
        rows
    }

    /// The worst state of the row in the column, how many hosts are in that state and how many
    /// hosts the column's task touched. `None` = no result for this row in this column.
    fn cell(&self, state: &RunState, col: &Column, row: &str) -> Option<(RowState, usize, usize)> {
        let hosts_total = {
            let mut hs: Vec<&str> = state
                .items
                .iter()
                .filter(|i| task_matches(&col.task, &i.task))
                .map(|i| i.host.as_str())
                .collect();
            hs.sort();
            hs.dedup();
            hs.len()
        };
        let states: Vec<(&str, RowState)> = state
            .items
            .iter()
            .filter(|i| task_matches(&col.task, &i.task) && i.item == row)
            .map(|i| (i.host.as_str(), row_state(i.changed, i.failed, i.skipped)))
            .collect();
        let worst = states.iter().map(|(_, s)| *s).max_by_key(|s| rank(*s))?;
        let mut hosts: Vec<&str> = states
            .iter()
            .filter(|(_, s)| *s == worst)
            .map(|(h, _)| *h)
            .collect();
        hosts.sort();
        hosts.dedup();
        Some((worst, hosts.len(), hosts_total))
    }

    /// True once some host reported the column's task (loop items or the task itself).
    fn column_ran(&self, state: &RunState, col: &Column) -> bool {
        state.items.iter().any(|i| task_matches(&col.task, &i.task))
            || state
                .results
                .iter()
                .any(|r| task_matches(&col.task, &r.task))
    }
}

impl Scene {
    pub(super) fn render_matrix(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let Some(m) = &self.matrix else { return };
        if area.height == 0 {
            return;
        }
        let bottom = area.y + area.height;
        let mut y = area.y;
        if !m.stages.is_empty() {
            let spans = self.stage_spans(state, &m.stages);
            buf.set_line(area.x, y, &Line::from(spans), area.width);
            y += 2;
        }
        let dim = Style::default().add_modifier(Modifier::DIM);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let check = state.mode == RunMode::Check;
        let rows = m.row_names(state);
        let title = self
            .def
            .bind
            .get("row_title")
            .and_then(|v| v.as_str())
            .unwrap_or("Кто");
        let name_w = rows
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap_or(0)
            .max(title.chars().count());
        let widths: Vec<usize> = m
            .columns
            .iter()
            .map(|c| c.label.chars().count().max(5))
            .collect();
        let mut put = |y: &mut usize, line: Line| {
            if *y < bottom as usize {
                buf.set_line(area.x, *y as u16, &line, area.width);
                *y += 1;
            }
        };
        let mut yy = y as usize;

        let mut head = vec![Span::styled(format!("{title:<name_w$}"), bold)];
        for (c, w) in m.columns.iter().zip(&widths) {
            head.push(Span::styled(format!("  {:<w$}", c.label, w = *w), bold));
        }
        head.push(Span::styled("  Итог", bold));
        put(&mut yy, Line::from(head));

        if rows.is_empty() {
            put(
                &mut yy,
                Line::from(Span::styled("пока нет данных".to_string(), dim)),
            );
        }
        for row in &rows {
            let mut spans = vec![Span::styled(format!("{row:<name_w$}"), bold)];
            let mut worst: Option<RowState> = None;
            for (c, w) in m.columns.iter().zip(&widths) {
                let (text, color) = match m.cell(state, c, row) {
                    Some((st, n, total)) => {
                        worst = Some(match worst {
                            Some(prev) if rank(prev) >= rank(st) => prev,
                            _ => st,
                        });
                        let (glyph, color) = match st {
                            RowState::Changed => ("+", self.theme.running),
                            RowState::Ok => ("✓", self.theme.ok),
                            RowState::Failed => ("✗", self.theme.fail),
                            RowState::Skipped => ("–", self.theme.pending),
                        };
                        let t = if total > 1 {
                            format!("{glyph} {n}/{total}")
                        } else {
                            glyph.to_string()
                        };
                        (t, color)
                    }
                    None => ("·".to_string(), self.theme.pending),
                };
                spans.push(Span::styled(
                    format!("  {text:<w$}", w = *w),
                    Style::default().fg(color),
                ));
            }
            let (word, color) = match worst {
                Some(RowState::Failed) => ("ошибка", self.theme.fail),
                Some(RowState::Changed) if check => ("изменится", self.theme.running),
                Some(RowState::Changed) => ("изменён", self.theme.running),
                Some(_) => ("без изменений", self.theme.ok),
                None => ("ждёт", self.theme.pending),
            };
            spans.push(Span::styled(
                format!("  {word}"),
                Style::default().fg(color),
            ));
            put(&mut yy, Line::from(spans));
        }

        let mut warnings: Vec<Line> = Vec::new();
        for c in &m.columns {
            let Some(text) = &c.missing else { continue };
            if !m.column_ran(state, c) {
                continue;
            }
            for row in rows.iter().filter(|r| m.cell(state, c, r).is_none()) {
                warnings.push(Line::from(vec![
                    Span::styled("! ", Style::default().fg(self.theme.running)),
                    Span::raw(format!("{row}: {text}")),
                ]));
            }
        }
        if !warnings.is_empty() {
            put(&mut yy, Line::raw(""));
            for w in warnings {
                put(&mut yy, w);
            }
        }
        put(&mut yy, Line::raw(""));
        let legend = if check {
            "+ будет изменено   ✓ уже есть   ✗ ошибка   · нет данных   n/m — узлов из всех"
        } else {
            "+ изменено   ✓ уже было   ✗ ошибка   · нет данных   n/m — узлов из всех"
        };
        put(&mut yy, Line::from(Span::styled(legend.to_string(), dim)));
    }
}
