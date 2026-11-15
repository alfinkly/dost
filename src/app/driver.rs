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
                    Action::None => {}
                }
            }
            Some(line) = rx.recv() => app.on_stream(line),
            Some(res) = done_rx.recv() => {
                while let Ok(l) = rx.try_recv() {
                    app.on_stream(l);
                }
                match res {
                    Ok(outcome) => app.on_finished(outcome),
                    Err(e) => app.on_error(format!("{e:#}")),
                }
            }
            _ = tick.tick() => {}
        }
    }
    cancel.cancel();
    Ok(())
}
