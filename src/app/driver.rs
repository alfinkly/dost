//! Terminal setup and the event loop that connects `App` to crossterm and the runner.

use std::io::{stdout, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{Action, App};
use crate::assistant::{apply_patch, ask, load_config, revert_patch, Advice};
use crate::catalog::Project;
use crate::runner::{build_plan, run as run_ansible, RunOutcome};
use crate::store::Store;
use crate::stream::{StreamLine, CALLBACK_PLUGIN};

/// Puts the terminal into raw/alternate mode and restores it on drop, also after a panic.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<TerminalGuard> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(TerminalGuard)
    }
}

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// Run the interactive app. `start` optionally opens one command straight away.
pub async fn run(project: Project, store: Store, start: Option<&str>) -> Result<()> {
    let plugin_dir = tempfile::tempdir()?;
    std::fs::write(plugin_dir.path().join("dost_stream.py"), CALLBACK_PLUGIN)?;

    let mut app = App::new(project, store);
    if let Some(id) = start {
        if !app.select_command(id) {
            anyhow::bail!("no command with id `{id}` in this project");
        }
    }

    let _guard = TerminalGuard::new()?;
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(stdout()))?;
    let (tx, mut rx) = mpsc::channel::<StreamLine>(512);
    let (done_tx, mut done_rx) = mpsc::channel::<Result<RunOutcome>>(1);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut cancel = CancellationToken::new();
    let assistant_cfg = load_config(&app.project.root);
    let (help_tx, mut help_rx) = mpsc::channel::<Result<Advice, String>>(1);
    let mut help_cancel = CancellationToken::new();
    let quit_signal = CancellationToken::new();
    crate::signals::cancel_on_signals(quit_signal.clone());

    loop {
        terminal.draw(|f| app.draw(f))?;
        tokio::select! {
            ev = events.next() => {
                let Some(Ok(Event::Key(key))) = ev else { continue };
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match app.handle_key(key) {
                    Action::Quit => break,
                    Action::Cancel => cancel.cancel(),
                    Action::Start { command, mode } => {
                        cancel = CancellationToken::new();
                        let plan = build_plan(
                            &app.project.root,
                            &app.project.commands[command],
                            mode,
                            plugin_dir.path(),
                        );
                        let pw = app.password();
                        let (tx, done_tx, token) = (tx.clone(), done_tx.clone(), cancel.clone());
                        tokio::spawn(async move {
                            let r = run_ansible(plan, pw, tx, token).await;
                            let _ = done_tx.send(r).await;
                        });
                    }
                    Action::AskHelp { prompt } => {
                        help_cancel = CancellationToken::new();
                        let (cfg, root, tx, token) = (
                            assistant_cfg.clone(),
                            app.project.root.clone(),
                            help_tx.clone(),
                            help_cancel.clone(),
                        );
                        tokio::spawn(async move {
                            let r = ask(&cfg, &prompt, &root, token).await.map_err(|e| e.to_string());
                            let _ = tx.send(r).await;
                        });
                    }
                    Action::CancelHelp => help_cancel.cancel(),
                    Action::ApplyPatch { diff } => {
                        let r = apply_patch(&app.project.root, &diff);
                        app.on_patch_result(r);
                    }
                    Action::RevertPatch { diff } => {
                        let r = revert_patch(&app.project.root, &diff);
                        app.on_patch_reverted(r);
                    }
                    Action::Copy { text } => app.on_copied(copy_to_clipboard(&text)),
                    Action::None => {}
                }
            }
            Some(line) = rx.recv() => app.on_stream(line),
            Some(advice) = help_rx.recv() => app.on_advice(advice),
            Some(res) = done_rx.recv() => {
                while let Ok(l) = rx.try_recv() {
                    app.on_stream(l);
                }
                match res {
                    Ok(outcome) => app.on_finished(outcome),
                    Err(e) => app.on_error(format!("{e:#}")),
                }
            }
            _ = quit_signal.cancelled() => {
                // SIGTERM/SIGHUP (e.g. the terminal window closed): stop Ansible, then leave.
                cancel.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(6), done_rx.recv()).await;
                break;
            }
            _ = tick.tick() => app.tick(),
        }
    }
    cancel.cancel();
    help_cancel.cancel();
    Ok(())
}

/// Put text on the clipboard with whatever the system has (pbcopy, wl-copy, xclip).
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidates: [(&str, &[&str]); 3] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
    ];
    for (prog, args) in candidates {
        let Ok(mut child) = Command::new(prog).args(args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return match child.wait() {
            Ok(s) if s.success() => Ok(()),
            _ => Err(format!("{prog} failed")),
        };
    }
    Err("no clipboard tool found (pbcopy, wl-copy or xclip)".into())
}
