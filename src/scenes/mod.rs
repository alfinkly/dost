//! Level-2 scenes: YAML definitions that turn a `RunState` into a picture built from widgets.

mod bind;

use std::collections::BTreeMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::catalog::{Project, SceneDef};
use crate::stream::{RunMode, RunState};
use crate::theme::Theme;
use crate::widgets::{
    Cell, ChainWidget, LinkState, MapLink, MapNode, MapWidget, NodeStatus, PipelineLink,
    PipelineWidget, TableWidget, TransferRow, TransferWidget, TreeNode, TreeWidget,
};

pub use bind::{parse_path, resolve_path, BindError, Seg};

pub const PREVIEW_LABEL: &str = "ПРОВЕРКА, предварительно";

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("scene `{id}`: unknown kind `{kind}` (expected chain, table, map or results)")]
    UnknownKind { id: String, kind: String },
    #[error("scene `{id}`: {source}")]
    Bind { id: String, source: BindError },
    #[error("scene `{id}`: table needs bind.rows")]
    MissingRows { id: String },
    #[error("scene `{id}`: tree: {reason}")]
    Tree { id: String, reason: String },
    #[error("scene `{id}`: steps: {reason}")]
    Steps { id: String, reason: String },
    #[error("scene `{id}`: report: {reason}")]
    Report { id: String, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Table,
    Map,
    Results,
    Tree,
    Report,
    Transfer,
}

#[derive(Debug, Clone)]
pub struct Scene {
    def: SceneDef,
    kind: Kind,
    theme: Theme,
    tree: Vec<TreeDef>,
    steps: Vec<StepDef>,
    report: Option<Report>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepDef {
    id: String,
    label: String,
}

fn parse_steps(def: &SceneDef) -> Result<Vec<StepDef>, SceneError> {
    let err = |reason: String| SceneError::Steps {
        id: def.id.clone(),
        reason,
    };
    let Some(raw) = def.bind.get("steps") else {
        return Ok(Vec::new());
    };
    let steps: Vec<StepDef> =
        serde_json::from_value(raw.clone()).map_err(|e| err(format!("bind.steps: {e}")))?;
    if steps.is_empty() {
        return Err(err("bind.steps is empty".into()));
    }
    for (i, s) in steps.iter().enumerate() {
        if steps[..i].iter().any(|o| o.id == s.id) {
            return Err(err(format!("duplicate step id `{}`", s.id)));
        }
    }
    Ok(steps)
}

#[derive(Debug, Clone, Deserialize)]
struct StageDef {
    play: String,
    label: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RuleDef {
    task: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    changed: Option<String>,
    #[serde(default)]
    ok: Option<String>,
    #[serde(default)]
    failed: Option<String>,
    #[serde(default)]
    skipped: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SectionDef {
    #[allow(dead_code)]
    id: String,
    title: String,
    #[serde(default)]
    play: Option<String>,
    rules: Vec<RuleDef>,
}

#[derive(Debug, Clone)]
struct Rule {
    task: Regex,
    label: String,
    words: [Option<String>; 4], // changed, ok, failed, skipped
}

#[derive(Debug, Clone)]
struct Section {
    title: String,
    play: Option<Regex>,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Report {
    stages: Vec<(Regex, String)>,
    sections: Vec<Section>,
}

fn parse_report(def: &SceneDef) -> Result<Report, SceneError> {
    let err = |reason: String| SceneError::Report {
        id: def.id.clone(),
        reason,
    };
    let re = |p: &str| Regex::new(p).map_err(|e| err(format!("invalid regex `{p}`: {e}")));
    let raw_sections: Vec<SectionDef> = serde_json::from_value(
        def.bind
            .get("sections")
            .cloned()
            .ok_or_else(|| err("needs bind.sections".into()))?,
    )
    .map_err(|e| err(format!("bind.sections: {e}")))?;
    if raw_sections.is_empty() {
        return Err(err("bind.sections is empty".into()));
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
    let mut sections = Vec::new();
    for s in raw_sections {
        let mut rules = Vec::new();
        for r in s.rules {
            rules.push(Rule {
                task: re(&r.task)?,
                label: r.label.unwrap_or_else(|| "{task}".into()),
                words: [r.changed, r.ok, r.failed, r.skipped],
            });
        }
        sections.push(Section {
            title: s.title,
            play: s.play.as_deref().map(re).transpose()?,
            rules,
        });
    }
    Ok(Report { stages, sections })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    Changed,
    Ok,
    Failed,
    Skipped,
}

struct Row {
    host: String,
    label: String,
    state: RowState,
    word: String,
}

fn row_state(changed: bool, failed: bool, skipped: bool) -> RowState {
    if failed {
        RowState::Failed
    } else if skipped {
        RowState::Skipped
    } else if changed {
        RowState::Changed
    } else {
        RowState::Ok
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TreeDef {
    id: String,
    label: String,
    #[serde(default)]
    parent: Option<String>,
}

fn parse_tree(def: &SceneDef) -> Result<Vec<TreeDef>, SceneError> {
    let err = |reason: String| SceneError::Tree {
        id: def.id.clone(),
        reason,
    };
    let raw = def
        .bind
        .get("nodes")
        .cloned()
        .ok_or_else(|| err("needs bind.nodes".into()))?;
    let nodes: Vec<TreeDef> =
        serde_json::from_value(raw).map_err(|e| err(format!("bind.nodes: {e}")))?;
    if nodes.is_empty() {
        return Err(err("bind.nodes is empty".into()));
    }
    for (i, n) in nodes.iter().enumerate() {
        if nodes[..i].iter().any(|o| o.id == n.id) {
            return Err(err(format!("duplicate node id `{}`", n.id)));
        }
        if let Some(p) = &n.parent {
            if !nodes.iter().any(|o| &o.id == p) {
                return Err(err(format!("node `{}` has unknown parent `{p}`", n.id)));
            }
        }
    }
    for n in &nodes {
        let mut cur = n;
        for _ in 0..=nodes.len() {
            match cur
                .parent
                .as_ref()
                .and_then(|p| nodes.iter().find(|o| &o.id == p))
            {
                Some(next) => cur = next,
                None => break,
            }
            if cur.id == n.id {
                return Err(err(format!("node `{}` is its own ancestor", n.id)));
            }
        }
    }
    Ok(nodes)
}

fn bind_str<'a>(def: &'a SceneDef, key: &str) -> Option<&'a str> {
    def.bind.get(key).and_then(Value::as_str)
}

impl Scene {
    pub fn from_def(def: &SceneDef) -> Result<Scene, SceneError> {
        Scene::with_theme(def, Theme::default())
    }

    pub fn with_theme(def: &SceneDef, theme: Theme) -> Result<Scene, SceneError> {
        let kind = match def.use_.as_str() {
            "chain" => Kind::Chain,
            "table" => Kind::Table,
            "map" => Kind::Map,
            "results" => Kind::Results,
            "tree" => Kind::Tree,
            "report" => Kind::Report,
            "transfer" => Kind::Transfer,
            other => {
                return Err(SceneError::UnknownKind {
                    id: def.id.clone(),
                    kind: other.to_string(),
                })
            }
        };
        if let Some(obj) = def.bind.as_object() {
            for (k, v) in obj {
                if k == "columns" {
                    continue;
                }
                if let Some(path) = v.as_str() {
                    parse_path(path).map_err(|source| SceneError::Bind {
                        id: def.id.clone(),
                        source,
                    })?;
                }
            }
        }
        if kind == Kind::Table && bind_str(def, "rows").is_none() {
            return Err(SceneError::MissingRows { id: def.id.clone() });
        }
        let tree = if kind == Kind::Tree {
            parse_tree(def)?
        } else {
            Vec::new()
        };
        let steps = if kind == Kind::Chain {
            parse_steps(def)?
        } else {
            Vec::new()
        };
        let report = if kind == Kind::Report {
            Some(parse_report(def)?)
        } else {
            None
        };
        Ok(Scene {
            def: def.clone(),
            kind,
            theme,
            tree,
            steps,
            report,
        })
    }

    pub fn id(&self) -> &str {
        &self.def.id
    }

    fn stage_status(&self, state: &RunState, play: &Regex) -> NodeStatus {
        let failed = state
            .results
            .iter()
            .any(|r| play.is_match(&r.play) && (r.failed || r.unreachable));
        if failed || (state.finished && state.any_failed() && play.is_match(&state.current_play)) {
            NodeStatus::Failed
        } else if play.is_match(&state.current_play) && !state.finished {
            NodeStatus::Running
        } else if state.plays.iter().any(|p| play.is_match(p)) {
            NodeStatus::Ok
        } else {
            NodeStatus::Pending
        }
    }

    fn report_rows(&self, state: &RunState, section: &Section) -> Vec<Row> {
        let allowed = |h: &str| state.focus.is_empty() || state.focus.iter().any(|f| f == h);
        let mut rows: Vec<Row> = Vec::new();
        for rule in &section.rules {
            let fill = |tpl: &str, host: &str, task: &str, item: &str| {
                tpl.replace("{item}", item)
                    .replace("{host}", host)
                    .replace("{task}", task)
            };
            let items: Vec<_> = state
                .items
                .iter()
                .filter(|i| rule.task.is_match(&i.task) && allowed(&i.host))
                .collect();
            let mut found: Vec<(String, String, RowState)> = Vec::new();
            if items.is_empty() {
                for r in state
                    .results
                    .iter()
                    .filter(|r| rule.task.is_match(&r.task) && allowed(&r.host))
                {
                    found.push((
                        r.host.clone(),
                        fill(&rule.label, &r.host, &r.task, ""),
                        row_state(r.changed, r.failed || r.unreachable, r.skipped),
                    ));
                }
            } else {
                for i in items {
                    found.push((
                        i.host.clone(),
                        fill(&rule.label, &i.host, &i.task, &i.item),
                        row_state(i.changed, i.failed, i.skipped),
                    ));
                }
            }
            for (host, label, st) in found {
                let word = rule.words[match st {
                    RowState::Changed => 0,
                    RowState::Ok => 1,
                    RowState::Failed => 2,
                    RowState::Skipped => 3,
                }]
                .clone()
                .unwrap_or_else(|| {
                    match st {
                        RowState::Changed => "changed",
                        RowState::Ok => "already",
                        RowState::Failed => "failed",
                        RowState::Skipped => "skipped",
                    }
                    .into()
                });
                rows.retain(|r| !(r.host == host && r.label == label));
                rows.push(Row {
                    host,
                    label,
                    state: st,
                    word,
                });
            }
        }
        rows
    }

    /// Stages of the run as a header, then sections of rows built from task and item results
    /// by the scene's rules; `focus` limits the rows to the named hosts.
    fn render_report(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let Some(report) = &self.report else { return };
        if area.height == 0 {
            return;
        }
        let mut spans: Vec<Span> = Vec::new();
        for (i, (play, label)) in report.stages.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ─▶ "));
            }
            let st = self.stage_status(state, play);
            spans.push(Span::styled(
                st.glyph_at(state.frame),
                st.style(&self.theme),
            ));
            spans.push(Span::raw(format!(" {label}")));
        }
        if !spans.is_empty() {
            buf.set_line(area.x, area.y, &Line::from(spans), area.width);
        }
        let body_y = area.y + if report.stages.is_empty() { 0 } else { 2 };
        if body_y >= area.y + area.height {
            return;
        }
        let body = Rect::new(area.x, body_y, area.width, area.y + area.height - body_y);
        let all: Vec<(&Section, Vec<Row>)> = report
            .sections
            .iter()
            .map(|s| (s, self.report_rows(state, s)))
            .collect();
        let mut hosts: Vec<&str> = all
            .iter()
            .flat_map(|(_, rows)| rows.iter().map(|r| r.host.as_str()))
            .collect();
        hosts.sort();
        hosts.dedup();
        let prefix = hosts.len() > 1;
        let ncols = if body.width >= 80 { 2 } else { 1 };
        let col_w = body.width / ncols as u16;
        let mut columns: Vec<Vec<Line>> = vec![Vec::new(); ncols];
        for (idx, (section, rows)) in all.iter().enumerate() {
            let col = &mut columns[idx % ncols];
            if !col.is_empty() {
                col.push(Line::raw(""));
            }
            let running = section
                .play
                .as_ref()
                .is_some_and(|p| p.is_match(&state.current_play))
                && !state.finished;
            let mut head = vec![Span::styled(
                section.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )];
            if running {
                head.push(Span::styled(
                    format!(" {}", NodeStatus::Running.glyph_at(state.frame)),
                    NodeStatus::Running.style(&self.theme),
                ));
            }
            col.push(Line::from(head));
            for r in rows {
                let (glyph, color) = match r.state {
                    RowState::Changed => ("+", self.theme.ok),
                    RowState::Ok => ("✓", self.theme.ok),
                    RowState::Failed => ("✗", self.theme.fail),
                    RowState::Skipped => ("–", self.theme.pending),
                };
                let label = if prefix {
                    format!("{}: {}", r.host, r.label)
                } else {
                    r.label.clone()
                };
                col.push(Line::from(vec![
                    Span::styled(glyph, Style::default().fg(color)),
                    Span::raw(format!(" {label}")),
                    Span::styled(
                        format!("  {}", r.word),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                ]));
            }
        }
        for (i, lines) in columns.into_iter().enumerate() {
            let rect = Rect::new(body.x + col_w * i as u16, body.y, col_w, body.height);
            for (k, line) in lines.iter().enumerate() {
                if k as u16 >= rect.height {
                    break;
                }
                buf.set_line(rect.x, rect.y + k as u16, line, rect.width);
            }
        }
    }

    /// Rows of `transfer` events (key = row id; `from`, `to`, `state`, optional `label`, `done`,
    /// `total` in bytes, `detail`), in the order they first arrived.
    fn render_transfer(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let titles: Vec<String> = self
            .def
            .bind
            .get("titles")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let titles = (
            titles.first().cloned().unwrap_or_else(|| "from".into()),
            titles.get(1).cloned().unwrap_or_else(|| "to".into()),
        );
        let strip: Vec<String> = self
            .def
            .bind
            .get("strip")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let clean = |s: String| strip.iter().fold(s, |acc, p| acc.replace(p.as_str(), ""));
        let rows = state
            .event_order
            .iter()
            .filter(|(_, kind, _)| kind == "transfer")
            .filter_map(|k| {
                let d = state.events.get(k)?;
                let text = |name: &str| d.get(name).and_then(Value::as_str).map(String::from);
                let st = text("state").unwrap_or_default();
                let status = match st.as_str() {
                    "running" => NodeStatus::Running,
                    "done" | "ok" => NodeStatus::Ok,
                    "failed" | "missing" | "refused" => NodeStatus::Failed,
                    "pending" => NodeStatus::Pending,
                    _ => NodeStatus::Unknown,
                };
                let detail = text("detail").or_else(|| match st.as_str() {
                    "missing" => Some("MISSING".into()),
                    "refused" => Some("still a prod address".into()),
                    "failed" => Some("failed".into()),
                    _ => None,
                });
                Some(TransferRow {
                    label: text("label").unwrap_or_else(|| k.2.clone()),
                    from: clean(text("from").unwrap_or_default()),
                    to: clean(text("to").unwrap_or_default()),
                    status,
                    done: d.get("done").and_then(Value::as_u64),
                    total: d.get("total").and_then(Value::as_u64),
                    detail,
                    age: state
                        .frame
                        .saturating_sub(state.event_frames.get(k).copied().unwrap_or(0)),
                })
            })
            .collect();
        TransferWidget {
            titles,
            rows,
            frame: state.frame,
            theme: self.theme.clone(),
        }
        .render(area, buf);
    }

    /// The node ids of a `tree` scene; `None` for other kinds.
    pub fn tree_node_ids(&self) -> Option<Vec<String>> {
        (self.kind == Kind::Tree).then(|| self.tree.iter().map(|n| n.id.clone()).collect())
    }

    fn render_tree(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let nodes = self
            .tree
            .iter()
            .map(|n| TreeNode {
                id: n.id.clone(),
                label: n.label.clone(),
                parent: n.parent.clone(),
                status: self.tree_status(state, &n.id),
            })
            .collect();
        TreeWidget {
            nodes,
            frame: state.frame,
            theme: self.theme.clone(),
        }
        .render(area, buf);
    }

    /// An event for the node wins; otherwise a focus node spins while the run lasts and is
    /// checked or crossed when it ends; everything else is unknown.
    fn tree_status(&self, state: &RunState, id: &str) -> NodeStatus {
        for ((_, kind, key), data) in &state.events {
            if kind == "node" && key == id {
                return match data.get("state").and_then(Value::as_str) {
                    Some("up") | Some("ok") => NodeStatus::Ok,
                    Some("down") | Some("failed") => NodeStatus::Failed,
                    Some("running") => NodeStatus::Running,
                    Some("pending") => NodeStatus::Pending,
                    _ => NodeStatus::Unknown,
                };
            }
        }
        if state.focus.iter().any(|f| f == id) {
            return if !state.finished {
                NodeStatus::Running
            } else if state.any_failed() {
                NodeStatus::Failed
            } else {
                NodeStatus::Ok
            };
        }
        NodeStatus::Unknown
    }

    pub fn render(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let check = state.mode == RunMode::Check;
        let mut inner = area;
        if check && area.height > 0 {
            buf.set_line(
                area.x,
                area.y,
                &Line::from(Span::styled(
                    PREVIEW_LABEL,
                    Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                )),
                area.width,
            );
            inner = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        }
        match self.kind {
            Kind::Chain => self.render_chain(state, inner, buf),
            Kind::Table => self.render_table(state, inner, buf),
            Kind::Map => self.render_map(state, inner, buf),
            Kind::Results => self.render_results(state, inner, buf),
            Kind::Tree => self.render_tree(state, inner, buf),
            Kind::Report => self.render_report(state, inner, buf),
            Kind::Transfer => self.render_transfer(state, inner, buf),
        }
        if check && !state.any_failed() {
            buf.set_style(inner, Style::default().add_modifier(Modifier::DIM));
        }
    }

    /// A fixed chain whose links are driven by `step` events (key = step id,
    /// `state: running|ok|failed`, optional `detail`).
    fn render_pipeline(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let links = self
            .steps
            .iter()
            .map(|s| {
                let ev = state
                    .events
                    .iter()
                    .find(|((_, kind, key), _)| kind == "step" && key == &s.id)
                    .map(|(_, d)| d);
                let mut status = match ev.and_then(|d| d.get("state")).and_then(Value::as_str) {
                    Some("running") => NodeStatus::Running,
                    Some("ok") | Some("up") => NodeStatus::Ok,
                    Some("failed") | Some("down") => NodeStatus::Failed,
                    _ => NodeStatus::Pending,
                };
                if status == NodeStatus::Running && state.finished {
                    status = NodeStatus::Failed;
                }
                let detail = ev
                    .and_then(|d| d.get("detail"))
                    .and_then(Value::as_str)
                    .map(String::from);
                PipelineLink {
                    label: s.label.clone(),
                    status,
                    detail,
                }
            })
            .collect();
        PipelineWidget {
            links,
            frame: state.frame,
            theme: self.theme.clone(),
        }
        .render(area, buf);
    }

    fn render_chain(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        if !self.steps.is_empty() {
            return self.render_pipeline(state, area, buf);
        }
        let mut names: Vec<&String> = Vec::new();
        for t in &state.tasks {
            if !names.contains(&t) {
                names.push(t);
            }
        }
        let last = names.len().saturating_sub(1);
        let steps = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let rs: Vec<_> = state.results.iter().filter(|r| &&r.task == name).collect();
                let status = if rs.iter().any(|r| r.failed || r.unreachable) {
                    NodeStatus::Failed
                } else if !rs.is_empty() {
                    NodeStatus::Ok
                } else if i == last {
                    NodeStatus::Running
                } else {
                    NodeStatus::Pending
                };
                ((*name).clone(), status)
            })
            .collect();
        ChainWidget {
            steps,
            theme: self.theme.clone(),
        }
        .render(area, buf);
    }

    fn host_context(state: &RunState, host: &str) -> Value {
        let mut event = serde_json::Map::new();
        for ((h, kind, key), data) in &state.events {
            if h != host {
                continue;
            }
            if key.is_empty() {
                event.insert(kind.clone(), data.clone());
            } else {
                let slot = event.entry(kind.clone()).or_insert_with(|| json!({}));
                if let Some(o) = slot.as_object_mut() {
                    o.insert(key.clone(), data.clone());
                }
            }
        }
        json!({ "host": host, "event": event })
    }

    fn render_table(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let rows_path = bind_str(&self.def, "rows").unwrap_or("");
        let was_path = bind_str(&self.def, "was");
        let columns: Vec<String> = self
            .def
            .bind
            .get("columns")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut y = area.y;
        for host in &state.hosts {
            let ctx = Self::host_context(state, host);
            let Some(now) = resolve_path(&ctx, rows_path)
                .ok()
                .and_then(|v| v.into_iter().next())
                .and_then(|v| v.as_object().cloned())
            else {
                continue;
            };
            let was = was_path
                .and_then(|p| resolve_path(&ctx, p).ok())
                .and_then(|v| v.into_iter().next())
                .and_then(|v| v.as_object().cloned());
            let mut keys: Vec<&String> = now.keys().collect();
            keys.sort_by_key(|k| (k.parse::<u64>().unwrap_or(u64::MAX), (*k).clone()));
            let show = |v: &Value| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            };
            let rows: Vec<Vec<Cell>> = keys
                .iter()
                .map(|k| {
                    let now_v = show(&now[*k]);
                    let mut row = vec![Cell {
                        text: (*k).clone(),
                        changed: false,
                    }];
                    match &was {
                        Some(w) => {
                            let was_v = w.get(*k).map(show);
                            let changed = was_v.as_deref().is_some_and(|x| x != now_v);
                            row.push(Cell {
                                text: was_v.unwrap_or_else(|| "—".into()),
                                changed: false,
                            });
                            row.push(Cell {
                                text: now_v,
                                changed,
                            });
                        }
                        None => row.push(Cell {
                            text: now_v,
                            changed: false,
                        }),
                    }
                    row
                })
                .collect();
            let width = if was.is_some() { 3 } else { 2 };
            let mut cols: Vec<String> = columns.iter().take(width).cloned().collect();
            let defaults = ["key", "was", "now"];
            while cols.len() < width {
                let i = cols.len();
                cols.push(if width == 2 && i == 1 {
                    "value".into()
                } else {
                    defaults[i].into()
                });
            }
            if y >= area.y + area.height {
                break;
            }
            buf.set_line(
                area.x,
                y,
                &Line::from(Span::styled(
                    host.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                area.width,
            );
            y += 1;
            let h = (rows.len() as u16 + 1).min((area.y + area.height).saturating_sub(y));
            let block = Rect::new(area.x, y, area.width, h);
            TableWidget {
                columns: cols,
                rows,
                highlight_changed: true,
                theme: self.theme.clone(),
            }
            .render(block, buf);
            y += h + 1;
        }
    }

    fn render_map(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let links_path = bind_str(&self.def, "links").unwrap_or("event.link[*]");
        let mut links: Vec<MapLink> = Vec::new();
        for host in &state.hosts {
            let ctx = Self::host_context(state, host);
            for l in resolve_path(&ctx, links_path).unwrap_or_default() {
                let (Some(a), Some(b)) = (
                    l.get("a").and_then(Value::as_str),
                    l.get("b").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let st = match l.get("state").and_then(Value::as_str) {
                    Some("up") => LinkState::Up,
                    Some("down") => LinkState::Down,
                    _ => LinkState::Unknown,
                };
                links.retain(|x| !(x.a == a && x.b == b));
                links.push(MapLink {
                    a: a.into(),
                    b: b.into(),
                    state: st,
                });
            }
        }
        let mut nodes: Vec<MapNode> = state
            .hosts
            .iter()
            .map(|h| {
                let rs: Vec<_> = state.results.iter().filter(|r| &r.host == h).collect();
                let status = if rs.iter().any(|r| r.failed || r.unreachable) {
                    NodeStatus::Failed
                } else if rs.is_empty() {
                    NodeStatus::Pending
                } else {
                    NodeStatus::Ok
                };
                MapNode {
                    id: h.clone(),
                    label: h.clone(),
                    status,
                }
            })
            .collect();
        for l in &links {
            for id in [&l.a, &l.b] {
                if !nodes.iter().any(|n| &n.id == id) {
                    nodes.push(MapNode {
                        id: id.clone(),
                        label: id.clone(),
                        status: NodeStatus::Pending,
                    });
                }
            }
        }
        MapWidget {
            nodes,
            links,
            theme: self.theme.clone(),
        }
        .render(area, buf);
    }

    fn render_results(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
        let changed: Vec<_> = state.results.iter().filter(|r| r.changed).collect();
        let mut lines = vec![Line::from(Span::styled(
            format!("{} changed", changed.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        for r in changed {
            let mut text = format!("● {}: {}", r.host, r.task);
            if let Some(first) = r.diff.as_deref().and_then(|d| d.lines().next()) {
                text.push_str(&format!("  {first}"));
            }
            lines.push(Line::from(text));
        }
        for (i, line) in lines.iter().enumerate() {
            if i as u16 >= area.height {
                break;
            }
            buf.set_line(area.x, area.y + i as u16, line, area.width);
        }
    }
}

const BUILTIN: [&str; 4] = [
    include_str!("../../assets/scenes/chain.yml"),
    include_str!("../../assets/scenes/ports.yml"),
    include_str!("../../assets/scenes/vpn.yml"),
    include_str!("../../assets/scenes/config.yml"),
];

pub fn builtin_defs() -> Vec<SceneDef> {
    BUILTIN
        .iter()
        .map(|t| serde_yaml_ng::from_str(t).expect("built-in scene must parse"))
        .collect()
}

/// Built-in scenes plus the project's own; a project scene replaces a built-in one with the same id.
pub fn load_scenes(project: &Project) -> Result<BTreeMap<String, Scene>, SceneError> {
    let theme = Theme::from_catalog(&project.theme);
    let mut out = BTreeMap::new();
    for def in builtin_defs().iter().chain(project.scenes.iter()) {
        out.insert(def.id.clone(), Scene::with_theme(def, theme.clone())?);
    }
    Ok(out)
}

/// Only the built-in scenes, used when a project scene is broken.
pub fn load_builtin_scenes(project: &Project) -> BTreeMap<String, Scene> {
    let theme = Theme::from_catalog(&project.theme);
    builtin_defs()
        .iter()
        .filter_map(|d| {
            Scene::with_theme(d, theme.clone())
                .ok()
                .map(|s| (d.id.clone(), s))
        })
        .collect()
}
