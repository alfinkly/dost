//! The "what is true now" message shown after a run.

use std::collections::BTreeMap;

use minijinja::{context, Environment};
use serde_json::{json, Value};

use crate::stream::RunState;

#[derive(Debug, thiserror::Error)]
pub enum SummaryError {
    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),
}

fn events_by_kind(state: &RunState) -> Value {
    let mut kinds: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    for ((host, kind, key), data) in &state.events {
        let per_host = kinds.entry(kind.clone()).or_default();
        if key.is_empty() {
            per_host.insert(host.clone(), data.clone());
        } else {
            let slot = per_host.entry(host.clone()).or_insert_with(|| json!({}));
            if let Some(obj) = slot.as_object_mut() {
                obj.insert(key.clone(), data.clone());
            }
        }
    }
    json!(kinds)
}

/// Render a catalog `done` template. Context: `hosts`, `stats`, `events` (kind → host → data;
/// a keyed event is nested under its key) and `results`.
pub fn render_done(template: &str, state: &RunState) -> Result<String, SummaryError> {
    let stats: BTreeMap<_, _> = state
        .recap
        .iter()
        .map(|(h, r)| {
            (
                h.clone(),
                json!({"ok": r.ok, "changed": r.changed, "failed": r.failed, "unreachable": r.unreachable}),
            )
        })
        .collect();
    let results: Vec<Value> = state
        .results
        .iter()
        .map(|r| {
            json!({"host": r.host, "task": r.task, "changed": r.changed, "failed": r.failed,
                   "skipped": r.skipped, "msg": r.msg})
        })
        .collect();
    let env = Environment::new();
    let out = env.render_str(
        template,
        context! {
            hosts => state.hosts.clone(),
            stats => stats,
            events => events_by_kind(state),
            results => results,
        },
    )?;
    Ok(out)
}

pub fn default_summary(state: &RunState) -> String {
    let changed = state.results.iter().filter(|r| r.changed).count();
    let failed = state
        .results
        .iter()
        .filter(|r| r.failed || r.unreachable)
        .count();
    format!(
        "Finished on {} hosts: {} changed, {} failed.",
        state.hosts.len(),
        changed,
        failed
    )
}

/// The catalog message when it renders, otherwise the default summary.
pub fn summary_for(done: Option<&str>, state: &RunState) -> String {
    match done.map(|t| render_done(t, state)) {
        Some(Ok(text)) => text,
        _ => default_summary(state),
    }
}
