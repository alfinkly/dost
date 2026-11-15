//! Output for when there is no terminal (pipe, CI): plain lines, no escape codes, no prompts.

use std::io::{BufRead, Write};

use anyhow::{bail, Context, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::catalog::{Command, Danger, Project};
use crate::hints::hints_for;
use crate::runner::{build_plan, run, RunOutcome};
use crate::secret::BecomePassword;
use crate::store::{playbook_hash, HistoryRecord, StepStatus, Store};
use crate::stream::{RunMode, RunState, StreamLine, CALLBACK_PLUGIN};
use crate::summary::summary_for;

/// One printable line for a stream line, if it deserves one.
pub fn format_line(line: &StreamLine) -> Option<String> {
    match line {
        StreamLine::TaskResult {
            host,
            task,
            changed,
            failed,
            unreachable,
            skipped,
            ..
        } => {
            let mark = if *failed || *unreachable {
                "FAILED "
            } else if *skipped {
                "skipped"
            } else if *changed {
                "changed"
            } else {
                "ok     "
            };
            Some(format!("{mark} {host}: {task}"))
        }
        StreamLine::Event {
            host, kind, key, ..
        } => Some(format!(
            "event   {host}: {kind}{}",
            key.as_deref()
                .map(|k| format!(" [{k}]"))
                .unwrap_or_default()
        )),
        _ => None,
    }
}

/// Failure text, matching hints included.
pub fn failure_report(project: &Project, state: &RunState, tail: &str) -> String {
    let mut out = String::new();
    for r in state.results.iter().filter(|r| r.failed || r.unreachable) {
        out.push_str(&format!("FAILED {}: {}\n", r.host, r.task));
        if let Some(m) = &r.msg {
            out.push_str(&format!("  {m}\n"));
        }
    }
    for f in state.event_failures() {
        out.push_str(&format!("FAILED {}: {}\n", f.key, f.detail));
    }
    for l in tail
        .lines()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        out.push_str(&format!("  {l}\n"));
    }
    for h in hints_for(&project.errors, state, tail) {
        out.push_str(&format!("\nCause: {}\nFix:   {}\n", h.cause, h.fix));
        if let Some(l) = &h.link {
            out.push_str(&format!("See:   {l}\n"));
        }
    }
    out
}

fn read_password() -> Result<BecomePassword> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("read the sudo password from stdin")?;
    let line = line.trim_end_matches(['\n', '\r']).to_string();
    if line.is_empty() {
        bail!("this command needs a sudo password: pipe it on stdin (first line)");
    }
    let pw = BecomePassword::new(line.into_bytes());
    if !pw.is_ansible_safe() {
        bail!("Ansible strips spaces around the password; passwords with leading or trailing whitespace are not supported");
    }
    Ok(pw)
}

struct Step<'a> {
    project: &'a Project,
    store: &'a Store,
    cmd: &'a Command,
    mode: RunMode,
    plugin_dir: &'a std::path::Path,
}

async fn run_mode(
    step: Step<'_>,
    pw: Option<BecomePassword>,
    cancel: CancellationToken,
    out: &mut impl Write,
) -> Result<(RunState, RunOutcome)> {
    let Step {
        project,
        store,
        cmd,
        mode,
        plugin_dir,
    } = step;
    let plan = build_plan(&project.root, cmd, mode, plugin_dir);
    let (tx, mut rx) = mpsc::channel::<StreamLine>(256);
    let task = tokio::spawn(run(plan, pw, tx, cancel));
    let mut state = RunState {
        mode,
        ..RunState::default()
    };
    while let Some(line) = rx.recv().await {
        if let Some(text) = format_line(&line) {
            writeln!(out, "{text}")?;
        }
        state.apply(line);
    }
    let outcome = task.await??;
    store.append_history(&HistoryRecord::from_run(&cmd.id, &state, outcome.exit_code));
    Ok((state, outcome))
}

/// Run one command without a terminal. A check always runs first; `apply` applies after it passed.
/// Returns the process exit code.
pub async fn run_plain(
    project: &Project,
    store: &Store,
    id: &str,
    apply: bool,
    confirm: Option<&str>,
    out: &mut impl Write,
) -> Result<i32> {
    let cmd = project
        .commands
        .iter()
        .find(|c| c.id == id)
        .with_context(|| format!("no command with id `{id}`"))?;
    if apply && cmd.danger == Danger::High && confirm != Some(cmd.id.as_str()) {
        bail!("`{id}` is danger: high; pass --confirm {id} to apply it without a terminal");
    }
    let plugin_dir = tempfile::tempdir()?;
    std::fs::write(plugin_dir.path().join("dost_stream.py"), CALLBACK_PLUGIN)?;
    let cancel = CancellationToken::new();
    crate::signals::cancel_on_signals(cancel.clone());
    let pw = if cmd.needs_become {
        Some(read_password()?)
    } else {
        None
    };

    if cmd.check {
        writeln!(out, "== check: {} ==", cmd.title)?;
        let (state, outcome) = run_mode(
            Step {
                project,
                store,
                cmd,
                mode: RunMode::Check,
                plugin_dir: plugin_dir.path(),
            },
            pw.clone(),
            cancel.clone(),
            out,
        )
        .await?;
        if outcome.cancelled {
            writeln!(out, "Interrupted: ansible was stopped.")?;
            return Ok(130);
        }
        if outcome.exit_code != Some(0) || state.any_failed() {
            writeln!(out, "Check failed. Nothing was changed.")?;
            write!(
                out,
                "{}",
                failure_report(project, &state, &outcome.output_tail)
            )?;
            return Ok(outcome.exit_code.filter(|c| *c != 0).unwrap_or(1));
        }
        writeln!(out, "Check passed.")?;
        if !apply {
            writeln!(out, "Nothing was changed. Run again with --apply to apply.")?;
            return Ok(0);
        }
    } else if !apply {
        writeln!(
            out,
            "`{id}` has check disabled; pass --apply to run it without a check."
        )?;
        return Ok(0);
    }

    writeln!(out, "== apply: {} ==", cmd.title)?;
    let (state, outcome) = run_mode(
        Step {
            project,
            store,
            cmd,
            mode: RunMode::Apply,
            plugin_dir: plugin_dir.path(),
        },
        pw,
        cancel.clone(),
        out,
    )
    .await?;
    if outcome.cancelled {
        writeln!(
            out,
            "Interrupted: ansible was stopped; hosts may be partly changed."
        )?;
        return Ok(130);
    }
    if outcome.exit_code != Some(0) || state.any_failed() {
        writeln!(out, "The run failed.")?;
        write!(
            out,
            "{}",
            failure_report(project, &state, &outcome.output_tail)
        )?;
        return Ok(outcome.exit_code.filter(|c| *c != 0).unwrap_or(1));
    }
    let hash = playbook_hash(cmd, &project.root);
    for chain in &project.chains {
        if chain.steps.iter().any(|s| s == id) {
            store.mark_done(&chain.id, id, &hash);
        }
    }
    writeln!(out, "{}", summary_for(cmd.done.as_deref(), &state))?;
    Ok(0)
}

/// `dost status <chain>`
pub fn print_status(
    project: &Project,
    store: &Store,
    chain_id: &str,
    out: &mut impl Write,
) -> Result<()> {
    let chain = project
        .chains
        .iter()
        .find(|c| c.id == chain_id)
        .with_context(|| format!("no chain with id `{chain_id}`"))?;
    let progress = store.chain_progress(chain, |step| {
        project
            .commands
            .iter()
            .find(|c| c.id == step)
            .map(|c| playbook_hash(c, &project.root))
            .unwrap_or_default()
    });
    let done = progress
        .iter()
        .filter(|(_, s)| *s == StepStatus::Done)
        .count();
    writeln!(
        out,
        "{} ({}): {done}/{}",
        chain.title,
        chain.id,
        progress.len()
    )?;
    for (step, s) in progress {
        let word = match s {
            StepStatus::Done => "done",
            StepStatus::Todo => "todo",
            StepStatus::MaybeStale => "may be stale",
        };
        writeln!(out, "  {step}: {word}")?;
    }
    Ok(())
}

/// `dost history`: the last 20 runs.
pub fn print_history(store: &Store, out: &mut impl Write) -> Result<()> {
    let all = store.read_history();
    for r in all.iter().rev().take(20) {
        writeln!(
            out,
            "{} {} {} exit {}",
            r.ts,
            r.id,
            r.mode,
            r.exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".into())
        )?;
    }
    Ok(())
}
