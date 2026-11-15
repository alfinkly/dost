//! The terminal app: a state machine (`App`) plus drawing (`view`) and the event loop (`driver`).
//! `App` never touches the terminal or spawns processes, so it is tested with scripted keys.

mod driver;
mod view;

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;

use crate::catalog::{Danger, Project};
use crate::hints::{match_errors, Failure};
use crate::runner::RunOutcome;
use crate::scenes::{load_scenes, Scene};
use crate::secret::BecomePassword;
use crate::store::{playbook_hash, HistoryRecord, StepStatus, Store};
use crate::stream::{RunMode, RunState, StreamLine};

pub use driver::run;

#[derive(Debug)]
pub enum Action {
    None,
    Quit,
    Start { command: usize, mode: RunMode },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKind {
    Menu,
    Preflight,
    Password,
    Running,
    ApplyConfirm,
    CheckFailed,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Scene,
    Log,
    Raw,
}

#[derive(Debug, Clone, Copy)]
enum MenuItem {
    Chain(usize),
    Command(usize),
}

pub struct App {
    pub(crate) project: Project,
    pub(crate) store: Store,
    pub(crate) scenes: BTreeMap<String, Scene>,
    items: Vec<MenuItem>,
    pub(crate) selected: usize,
    pub(crate) screen: ScreenKind,
    pub(crate) cur: Option<usize>,
    pub(crate) typed: String,
    pub(crate) pw_len: usize,
    pw_input: Vec<u8>,
    password: Option<BecomePassword>,
    pending_mode: RunMode,
    pub(crate) forced: bool,
    pub(crate) skipped_check: bool,
    pub(crate) state: RunState,
    pub(crate) running_mode: RunMode,
    pub(crate) tab: Tab,
    pub(crate) last_exit: Option<i32>,
    pub(crate) last_tail: String,
    pub(crate) error: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) next_step: Option<String>,
}

impl App {
    pub fn new(project: Project, store: Store) -> App {
        let scenes = load_scenes(&project).unwrap_or_default();
        let mut items: Vec<MenuItem> = (0..project.chains.len()).map(MenuItem::Chain).collect();
        items.extend((0..project.commands.len()).map(MenuItem::Command));
        App {
            project,
            store,
            scenes,
            items,
            selected: 0,
            screen: ScreenKind::Menu,
            cur: None,
            typed: String::new(),
            pw_len: 0,
            pw_input: Vec::new(),
            password: None,
            pending_mode: RunMode::Check,
            forced: false,
            skipped_check: false,
            state: RunState::default(),
            running_mode: RunMode::Check,
            tab: Tab::Scene,
            last_exit: None,
            last_tail: String::new(),
            error: None,
            message: None,
            next_step: None,
        }
    }

    pub fn screen_kind(&self) -> ScreenKind {
        self.screen
    }

    /// A copy of the stored `become` password, for the next run.
    pub fn password(&self) -> Option<BecomePassword> {
        self.password.clone()
    }

    pub fn draw(&self, f: &mut Frame) {
        view::draw(self, f);
    }

    /// Jump straight to the confirmation of one command (used by `dost run <id>`).
    pub fn select_command(&mut self, id: &str) -> bool {
        match self.project.commands.iter().position(|c| c.id == id) {
            Some(i) => {
                self.open_command(i);
                true
            }
            None => false,
        }
    }

    pub(crate) fn command(&self) -> &crate::catalog::Command {
        &self.project.commands[self.cur.unwrap_or(0)]
    }

    pub(crate) fn menu_rows(&self) -> Vec<(bool, String, String)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let sel = i == self.selected;
                match it {
                    MenuItem::Chain(c) => {
                        let ch = &self.project.chains[*c];
                        let (done, total) = self.chain_counts(*c);
                        (
                            sel,
                            format!("chain {}  {done}/{total}", ch.id),
                            ch.title.clone(),
                        )
                    }
                    MenuItem::Command(c) => {
                        let cm = &self.project.commands[*c];
                        (sel, cm.title.clone(), cm.about.clone())
                    }
                }
            })
            .collect()
    }

    pub(crate) fn chain_progress(&self, c: usize) -> Vec<(String, StepStatus)> {
        let chain = &self.project.chains[c];
        self.store.chain_progress(chain, |step| {
            self.project
                .commands
                .iter()
                .find(|x| x.id == step)
                .map(|x| playbook_hash(x, &self.project.root))
                .unwrap_or_default()
        })
    }

    fn chain_counts(&self, c: usize) -> (usize, usize) {
        let p = self.chain_progress(c);
        (
            p.iter().filter(|(_, s)| *s == StepStatus::Done).count(),
            p.len(),
        )
    }

    pub(crate) fn failure(&self) -> Failure {
        let mut text: Vec<String> = self
            .state
            .results
            .iter()
            .filter(|r| r.failed || r.unreachable)
            .filter_map(|r| r.msg.clone())
            .collect();
        text.push(self.last_tail.clone());
        Failure {
            module: None,
            text: text.join("\n"),
        }
    }

    pub(crate) fn hints(&self) -> Vec<&crate::catalog::ErrorRule> {
        match_errors(&self.project.errors, &self.failure())
    }

    fn open_command(&mut self, idx: usize) {
        self.cur = Some(idx);
        self.typed.clear();
        self.message = None;
        self.screen = ScreenKind::Preflight;
    }

    fn back_to_menu(&mut self) {
        self.screen = ScreenKind::Menu;
        self.password = None;
        self.pw_input.clear();
        self.pw_len = 0;
        self.typed.clear();
        self.forced = false;
        self.skipped_check = false;
    }

    fn begin(&mut self, mode: RunMode) -> Action {
        if self.command().needs_become && self.password.is_none() {
            self.pending_mode = mode;
            self.pw_input.clear();
            self.pw_len = 0;
            self.screen = ScreenKind::Password;
            return Action::None;
        }
        self.start(mode)
    }

    fn start(&mut self, mode: RunMode) -> Action {
        self.state = RunState {
            mode,
            ..RunState::default()
        };
        self.running_mode = mode;
        self.screen = ScreenKind::Running;
        self.tab = Tab::Scene;
        self.error = None;
        self.last_tail.clear();
        Action::Start {
            command: self.cur.unwrap_or(0),
            mode,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        match self.screen {
            ScreenKind::Menu => self.key_menu(key, ctrl_c),
            ScreenKind::Preflight => match key.code {
                KeyCode::Enter => {
                    if self.command().check {
                        self.begin(RunMode::Check)
                    } else {
                        self.skipped_check = true;
                        self.forced = false;
                        self.typed.clear();
                        self.screen = ScreenKind::ApplyConfirm;
                        Action::None
                    }
                }
                KeyCode::Esc => {
                    self.back_to_menu();
                    Action::None
                }
                _ if ctrl_c => {
                    self.back_to_menu();
                    Action::None
                }
                _ => Action::None,
            },
            ScreenKind::Password => self.key_password(key, ctrl_c),
            ScreenKind::Running => match key.code {
                KeyCode::Tab => {
                    self.tab = match self.tab {
                        Tab::Scene => Tab::Log,
                        Tab::Log => Tab::Raw,
                        Tab::Raw => Tab::Scene,
                    };
                    Action::None
                }
                KeyCode::Char('q') | KeyCode::Esc => Action::Cancel,
                _ if ctrl_c => Action::Cancel,
                _ => Action::None,
            },
            ScreenKind::ApplyConfirm => self.key_apply_confirm(key, ctrl_c),
            ScreenKind::CheckFailed => match key.code {
                KeyCode::Char('a') => {
                    self.forced = true;
                    self.skipped_check = false;
                    self.typed.clear();
                    self.screen = ScreenKind::ApplyConfirm;
                    Action::None
                }
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                    self.back_to_menu();
                    Action::None
                }
                _ if ctrl_c => {
                    self.back_to_menu();
                    Action::None
                }
                _ => Action::None,
            },
            ScreenKind::Result => match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                    self.back_to_menu();
                    Action::None
                }
                _ if ctrl_c => {
                    self.back_to_menu();
                    Action::None
                }
                _ => Action::None,
            },
        }
    }

    fn key_menu(&mut self, key: KeyEvent, ctrl_c: bool) -> Action {
        if ctrl_c {
            return Action::Quit;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Enter => {
                match self.items.get(self.selected).copied() {
                    Some(MenuItem::Command(i)) => self.open_command(i),
                    Some(MenuItem::Chain(c)) => {
                        let progress = self.chain_progress(c);
                        let next = progress.iter().find(|(_, s)| *s != StepStatus::Done);
                        match next {
                            Some((step, _)) => {
                                let step = step.clone();
                                if let Some(i) =
                                    self.project.commands.iter().position(|x| x.id == step)
                                {
                                    self.open_command(i);
                                }
                            }
                            None => self.message = Some("Chain is complete.".into()),
                        }
                    }
                    None => {}
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    fn key_password(&mut self, key: KeyEvent, ctrl_c: bool) -> Action {
        if ctrl_c {
            self.back_to_menu();
            return Action::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.back_to_menu();
                Action::None
            }
            KeyCode::Backspace => {
                // drop one whole character (UTF-8 aware)
                if let Ok(s) = std::str::from_utf8(&self.pw_input) {
                    let mut chars: Vec<char> = s.chars().collect();
                    chars.pop();
                    self.pw_input = chars.into_iter().collect::<String>().into_bytes();
                }
                self.pw_len = self.pw_len.saturating_sub(1);
                Action::None
            }
            KeyCode::Enter => {
                if self.pw_input.is_empty() {
                    return Action::None;
                }
                let bytes = std::mem::take(&mut self.pw_input);
                self.password = Some(BecomePassword::new(bytes));
                self.pw_len = 0;
                let mode = self.pending_mode;
                self.start(mode)
            }
            KeyCode::Char(c) => {
                let mut buf = [0u8; 4];
                self.pw_input
                    .extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                self.pw_len += 1;
                Action::None
            }
            _ => Action::None,
        }
    }

    fn key_apply_confirm(&mut self, key: KeyEvent, ctrl_c: bool) -> Action {
        if ctrl_c {
            self.back_to_menu();
            return Action::None;
        }
        let required = self.forced || self.command().danger != Danger::Low;
        match key.code {
            KeyCode::Esc => {
                self.back_to_menu();
                Action::None
            }
            KeyCode::Backspace => {
                self.typed.pop();
                Action::None
            }
            KeyCode::Enter => {
                if required && self.typed != self.command().id {
                    self.message = Some(format!("Type `{}` to continue.", self.command().id));
                    return Action::None;
                }
                self.message = None;
                self.begin(RunMode::Apply)
            }
            KeyCode::Char(c) => {
                self.typed.push(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    pub fn on_stream(&mut self, line: StreamLine) {
        self.state.apply(line);
    }

    pub fn on_error(&mut self, message: String) {
        self.error = Some(message);
        self.screen = ScreenKind::Result;
    }

    pub fn on_finished(&mut self, outcome: RunOutcome) {
        if outcome.cancelled {
            self.back_to_menu();
            self.message = Some("Run cancelled.".into());
            return;
        }
        self.last_exit = outcome.exit_code;
        self.last_tail = outcome.output_tail;
        let cmd_id = self.command().id.clone();
        self.store.append_history(&HistoryRecord::from_run(
            &cmd_id,
            &self.state,
            outcome.exit_code,
        ));
        let success = outcome.exit_code == Some(0) && !self.state.any_failed();
        match self.running_mode {
            RunMode::Check => {
                if success {
                    self.forced = false;
                    self.skipped_check = false;
                    self.typed.clear();
                    self.screen = ScreenKind::ApplyConfirm;
                } else {
                    self.screen = ScreenKind::CheckFailed;
                }
            }
            RunMode::Apply => {
                self.next_step = None;
                if success {
                    let hash = playbook_hash(self.command(), &self.project.root);
                    for chain in &self.project.chains {
                        if let Some(pos) = chain.steps.iter().position(|s| *s == cmd_id) {
                            self.store.mark_done(&chain.id, &cmd_id, &hash);
                            if let Some(next) = chain.steps.get(pos + 1) {
                                self.next_step =
                                    Some(format!("Next in chain `{}`: {next}", chain.id));
                            }
                        }
                    }
                }
                self.screen = ScreenKind::Result;
            }
        }
    }
}
