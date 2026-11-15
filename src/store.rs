//! Local memory of dost: chain progress and run history. Never holds secrets.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{Chain, Command};
use crate::stream::{RunMode, RunState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Todo,
    Done,
    MaybeStale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryTask {
    pub host: String,
    pub task: String,
    pub changed: bool,
    pub failed: bool,
}

/// One finished run. Only statuses are stored: no messages, diffs or passwords.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub ts: u64,
    pub id: String,
    pub mode: String,
    pub exit_code: Option<i32>,
    pub hosts: Vec<String>,
    pub tasks: Vec<HistoryTask>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl HistoryRecord {
    pub fn from_run(id: &str, state: &RunState, exit_code: Option<i32>) -> Self {
        HistoryRecord {
            ts: now(),
            id: id.to_string(),
            mode: match state.mode {
                RunMode::Check => "check".into(),
                RunMode::Apply => "apply".into(),
            },
            exit_code,
            hosts: state.hosts.clone(),
            tasks: state
                .results
                .iter()
                .map(|r| HistoryTask {
                    host: r.host.clone(),
                    task: r.task.clone(),
                    changed: r.changed,
                    failed: r.failed || r.unreachable,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StepMark {
    hash: String,
    ts: u64,
}

type ChainFile = BTreeMap<String, BTreeMap<String, StepMark>>;

pub struct Store {
    state_dir: PathBuf,
    history_dir: PathBuf,
}

impl Store {
    pub fn open(root: &Path) -> Store {
        let dost = root.join(".dost");
        Store {
            state_dir: dost.join("state"),
            history_dir: dost.join("history"),
        }
    }

    fn chains_path(&self) -> PathBuf {
        self.state_dir.join("chains.json")
    }

    fn load_chains(&self) -> ChainFile {
        fs::read_to_string(self.chains_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn mark_done(&self, chain: &str, step: &str, hash: &str) {
        let mut all = self.load_chains();
        all.entry(chain.to_string()).or_default().insert(
            step.to_string(),
            StepMark {
                hash: hash.to_string(),
                ts: now(),
            },
        );
        if fs::create_dir_all(&self.state_dir).is_ok() {
            if let Ok(text) = serde_json::to_string_pretty(&all) {
                let _ = fs::write(self.chains_path(), text);
            }
        }
    }

    pub fn chain_progress(
        &self,
        chain: &Chain,
        current_hash: impl Fn(&str) -> String,
    ) -> Vec<(String, StepStatus)> {
        let all = self.load_chains();
        let marks = all.get(&chain.id);
        chain
            .steps
            .iter()
            .map(|step| {
                let status = match marks.and_then(|m| m.get(step)) {
                    None => StepStatus::Todo,
                    Some(m) if m.hash == current_hash(step) => StepStatus::Done,
                    Some(_) => StepStatus::MaybeStale,
                };
                (step.clone(), status)
            })
            .collect()
    }

    pub fn append_history(&self, rec: &HistoryRecord) {
        if fs::create_dir_all(&self.history_dir).is_err() {
            return;
        }
        let Ok(line) = serde_json::to_string(rec) else {
            return;
        };
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.history_dir.join("runs.jsonl"))
        {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn read_history(&self) -> Vec<HistoryRecord> {
        fs::read_to_string(self.history_dir.join("runs.jsonl"))
            .map(|t| {
                t.lines()
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn hash_path(h: &mut Sha256, root: &Path, p: &Path) {
    if p.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(p)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        entries.sort();
        for e in entries {
            hash_path(h, root, &e);
        }
    } else if let Ok(bytes) = fs::read(p) {
        h.update(
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .as_bytes(),
        );
        h.update([0]);
        h.update(&bytes);
        h.update([0]);
    }
}

/// Hash of the playbook file plus every file matched by the command's `watch` globs.
pub fn playbook_hash(cmd: &Command, root: &Path) -> String {
    let mut h = Sha256::new();
    let pb = root.join(&cmd.playbook);
    match fs::read(&pb) {
        Ok(bytes) => h.update(&bytes),
        Err(_) => h.update(b"missing"),
    }
    for pattern in &cmd.watch {
        let full = root.join(pattern);
        let Ok(paths) = glob::glob(&full.to_string_lossy()) else {
            continue;
        };
        let mut matched: Vec<PathBuf> = paths.filter_map(Result::ok).collect();
        matched.sort();
        for m in matched {
            hash_path(&mut h, root, &m);
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
