//! Level-2 scenes: YAML definitions that turn a `RunState` into a picture built from widgets.

mod bind;

use std::collections::BTreeMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use serde_json::{json, Value};

use crate::catalog::{Project, SceneDef};
use crate::stream::{RunMode, RunState};
use crate::theme::Theme;
use crate::widgets::{
    Cell, ChainWidget, LinkState, MapLink, MapNode, MapWidget, NodeStatus, TableWidget,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chain,
    Table,
    Map,
    Results,
}

#[derive(Debug, Clone)]
pub struct Scene {
    def: SceneDef,
    kind: Kind,
    theme: Theme,
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
        Ok(Scene {
            def: def.clone(),
            kind,
            theme,
        })
    }

    pub fn id(&self) -> &str {
        &self.def.id
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
        }
        if check {
            buf.set_style(inner, Style::default().add_modifier(Modifier::DIM));
        }
    }

    fn render_chain(&self, state: &RunState, area: Rect, buf: &mut Buffer) {
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
