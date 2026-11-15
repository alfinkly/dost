//! Parsing of the JSON-lines stream produced by the bundled `dost_stream` callback,
//! and the run state built from it.

use std::collections::BTreeMap;

use serde_json::Value;

/// Source of the callback plugin, written to a temporary directory at run time.
pub const CALLBACK_PLUGIN: &str = include_str!("../assets/dost_stream.py");

const MAX_LINE: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    Check,
    #[default]
    Apply,
}

#[derive(Debug, Clone)]
pub enum StreamLine {
    RunStart {
        mode: RunMode,
    },
    Play {
        name: String,
    },
    Item {
        host: String,
        task: String,
        item: String,
        changed: bool,
        failed: bool,
        skipped: bool,
    },
    TaskStart {
        name: String,
    },
    TaskResult {
        host: String,
        task: String,
        changed: bool,
        failed: bool,
        skipped: bool,
        unreachable: bool,
        module: Option<String>,
        msg: Option<String>,
        diff: Option<String>,
    },
    Event {
        host: String,
        kind: String,
        key: Option<String>,
        data: Value,
    },
    Recap {
        host: String,
        ok: u32,
        changed: u32,
        failed: u32,
        unreachable: u32,
    },
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn n(v: &Value, k: &str) -> u32 {
    v.get(k)
        .and_then(Value::as_u64)
        .map(|x| x.min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

/// Parse one line. Returns `None` for anything that is not a well-formed stream line.
pub fn parse_line(line: &str) -> Option<StreamLine> {
    if line.len() > MAX_LINE {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    match v.get("t")?.as_str()? {
        "start" => {
            let mode = match v.get("mode").and_then(Value::as_str) {
                Some("check") => RunMode::Check,
                _ => RunMode::Apply,
            };
            Some(StreamLine::RunStart { mode })
        }
        "play" => Some(StreamLine::Play {
            name: s(&v, "name")?,
        }),
        "item" => Some(StreamLine::Item {
            host: s(&v, "host")?,
            task: s(&v, "task")?,
            item: if b(&v, "no_log") {
                String::new()
            } else {
                s(&v, "item").unwrap_or_default()
            },
            changed: b(&v, "changed"),
            failed: b(&v, "failed"),
            skipped: b(&v, "skipped"),
        }),
        "task" => Some(StreamLine::TaskStart {
            name: s(&v, "name")?,
        }),
        "result" => {
            let no_log = b(&v, "no_log");
            Some(StreamLine::TaskResult {
                host: s(&v, "host")?,
                task: s(&v, "task")?,
                changed: b(&v, "changed"),
                failed: b(&v, "failed"),
                skipped: b(&v, "skipped"),
                unreachable: b(&v, "unreachable"),
                module: s(&v, "module"),
                msg: if no_log { None } else { s(&v, "msg") },
                diff: if no_log { None } else { s(&v, "diff") },
            })
        }
        "event" => {
            if v.get("v").and_then(Value::as_u64) != Some(1) {
                return None;
            }
            Some(StreamLine::Event {
                host: s(&v, "host")?,
                kind: s(&v, "kind")?,
                key: s(&v, "key"),
                data: v.get("data").cloned().unwrap_or(Value::Null),
            })
        }
        "recap" => Some(StreamLine::Recap {
            host: s(&v, "host")?,
            ok: n(&v, "ok"),
            changed: n(&v, "changed"),
            failed: n(&v, "failed"),
            unreachable: n(&v, "unreachable"),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recap {
    pub ok: u32,
    pub changed: u32,
    pub failed: u32,
    pub unreachable: u32,
}

#[derive(Debug, Clone)]
pub struct TaskResultRec {
    pub play: String,
    pub host: String,
    pub task: String,
    pub changed: bool,
    pub failed: bool,
    pub skipped: bool,
    pub unreachable: bool,
    pub module: Option<String>,
    pub msg: Option<String>,
    pub diff: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ItemRec {
    pub play: String,
    pub host: String,
    pub task: String,
    pub item: String,
    pub changed: bool,
    pub failed: bool,
    pub skipped: bool,
}

/// Everything known about one run so far.
#[derive(Debug, Clone, Default)]
pub struct RunState {
    pub mode: RunMode,
    /// Tree nodes the command works on (set by the app, kept across `RunStart`).
    pub focus: Vec<String>,
    /// Animation frame, advanced by the app on every tick.
    pub frame: u64,
    /// True once Ansible printed its recap (or the app saw the process end).
    pub finished: bool,
    pub hosts: Vec<String>,
    pub tasks: Vec<String>,
    /// Names of the plays seen so far, and the one running now.
    pub plays: Vec<String>,
    pub current_play: String,
    pub results: Vec<TaskResultRec>,
    /// Results of single loop items (the label is the loop item).
    pub items: Vec<ItemRec>,
    /// Keyed by (host, kind, key); same key overwrites, different keys accumulate.
    pub events: BTreeMap<(String, String, String), Value>,
    pub recap: BTreeMap<String, Recap>,
}

impl RunState {
    fn see_host(&mut self, host: &str) {
        if !self.hosts.iter().any(|h| h == host) {
            self.hosts.push(host.to_string());
        }
    }

    pub fn apply(&mut self, line: StreamLine) {
        match line {
            StreamLine::RunStart { mode } => {
                *self = RunState {
                    mode,
                    focus: std::mem::take(&mut self.focus),
                    frame: self.frame,
                    ..RunState::default()
                };
            }
            StreamLine::Play { name } => {
                self.current_play = name.clone();
                self.plays.push(name);
            }
            StreamLine::Item {
                host,
                task,
                item,
                changed,
                failed,
                skipped,
            } => {
                self.see_host(&host);
                self.items.push(ItemRec {
                    play: self.current_play.clone(),
                    host,
                    task,
                    item,
                    changed,
                    failed,
                    skipped,
                });
            }
            StreamLine::TaskStart { name } => self.tasks.push(name),
            StreamLine::TaskResult {
                host,
                task,
                changed,
                failed,
                skipped,
                unreachable,
                module,
                msg,
                diff,
            } => {
                self.see_host(&host);
                self.results.push(TaskResultRec {
                    play: self.current_play.clone(),
                    host,
                    task,
                    changed,
                    failed,
                    skipped,
                    unreachable,
                    module,
                    msg,
                    diff,
                });
            }
            StreamLine::Event {
                host,
                kind,
                key,
                data,
            } => {
                self.see_host(&host);
                self.events
                    .insert((host, kind, key.unwrap_or_default()), data);
            }
            StreamLine::Recap {
                host,
                ok,
                changed,
                failed,
                unreachable,
            } => {
                self.see_host(&host);
                self.finished = true;
                self.recap.insert(
                    host,
                    Recap {
                        ok,
                        changed,
                        failed,
                        unreachable,
                    },
                );
            }
        }
    }

    pub fn any_failed(&self) -> bool {
        self.results.iter().any(|r| r.failed || r.unreachable)
    }
}
