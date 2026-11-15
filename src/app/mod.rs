//! The terminal app: a state machine (`App`) plus drawing (`view`) and the event loop (`driver`).
//! `App` never touches the terminal or spawns processes, so it is tested with scripted keys.

mod driver;
mod view;

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;

use crate::assistant::{build_prompt, Advice, HelpContext, Proposal};
use crate::catalog::{Danger, Project};
use crate::hints::hints_for;
use crate::runner::RunOutcome;
use crate::scenes::{load_builtin_scenes, load_scenes, Scene};
use crate::secret::BecomePassword;
use crate::store::{playbook_hash, HistoryRecord, StepStatus, Store};
use crate::stream::{RunMode, RunState, StreamLine};

pub use driver::run;

#[derive(Debug)]
pub enum Action {
    None,
    Quit,
    Start {
        command: usize,
        mode: RunMode,
    },
    Cancel,
    /// Send this prompt to the headless assistant.
    AskHelp {
        prompt: String,
    },
    CancelHelp,
    ApplyPatch {
        diff: String,
    },
    RevertPatch {
        diff: String,
    },
    /// Put this text on the clipboard.
    Copy {
        text: String,
    },
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
    HelpConfirm,
    HelpAsking,
    HelpAdvice,
    HelpDetail,
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
    help_return: ScreenKind,
    help_consent: bool,
    pub(crate) help_prompt: String,
    pub(crate) advice: Option<Advice>,
    pub(crate) adv_sel: usize,
    pub(crate) step_sel: usize,
    pending_patch: Option<String>,
    last_patch: Option<String>,
}

impl App {
    pub fn new(project: Project, store: Store) -> App {
        let (scenes, scene_problem) = match load_scenes(&project) {
            Ok(s) => (s, None),
            Err(e) => (
                load_builtin_scenes(&project),
                Some(format!("Scene problem: {e}. Using the built-in scenes.")),
            ),
        };
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
            message: scene_problem,
            next_step: None,
            help_return: ScreenKind::Menu,
            help_consent: false,
            help_prompt: String::new(),
            advice: None,
            adv_sel: 0,
            step_sel: 0,
            pending_patch: None,
            last_patch: None,
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

    /// True when the failure says the sudo password was wrong or missing.
    fn password_rejected(&self) -> bool {
        const MARKERS: [&str; 3] = [
            "incorrect sudo password",
            "missing sudo password",
            "a password is required",
        ];
        let mut texts: Vec<String> = self
            .state
            .results
            .iter()
            .filter(|r| r.failed || r.unreachable)
            .filter_map(|r| r.msg.clone())
            .collect();
        texts.extend(self.state.event_failures().into_iter().map(|f| f.detail));
        texts.push(self.last_tail.clone());
        texts
            .iter()
            .any(|t| MARKERS.iter().any(|m| t.to_lowercase().contains(m)))
    }

    /// The last run ended without success (a failed task, a red link, a non-zero exit or an error).
    pub(crate) fn can_undo(&self) -> bool {
        self.last_patch.is_some()
    }

    fn help_context(&self) -> HelpContext {
        let mut failures: Vec<String> = self
            .state
            .results
            .iter()
            .filter(|r| r.failed || r.unreachable)
            .map(|r| match &r.msg {
                Some(m) => format!("✗ {}: {}: {m}", r.host, r.task),
                None => format!("✗ {}: {}", r.host, r.task),
            })
            .collect();
        failures.extend(
            self.state
                .event_failures()
                .into_iter()
                .map(|f| format!("✗ {}: {}", f.key, f.detail)),
        );
        HelpContext {
            command_id: self.command().id.clone(),
            title: self.command().title.clone(),
            about: self.command().about.clone(),
            playbook: self.command().playbook.clone(),
            mode: self.running_mode,
            failures,
            output_tail: self.last_tail.clone(),
            catalog_ids: self.project.commands.iter().map(|c| c.id.clone()).collect(),
        }
    }

    fn start_help(&mut self) -> Action {
        self.help_return = self.screen;
        self.help_prompt = build_prompt(&self.help_context());
        if self.help_consent {
            self.send_help()
        } else {
            self.screen = ScreenKind::HelpConfirm;
            Action::None
        }
    }

    fn send_help(&mut self) -> Action {
        self.screen = ScreenKind::HelpAsking;
        self.message = None;
        Action::AskHelp {
            prompt: self.help_prompt.clone(),
        }
    }

    fn undo_patch(&mut self) -> Action {
        match self.last_patch.clone() {
            Some(diff) => Action::RevertPatch { diff },
            None => Action::None,
        }
    }

    pub fn on_advice(&mut self, result: Result<Advice, String>) {
        match result {
            Ok(a) if !a.proposals.is_empty() => {
                self.advice = Some(a);
                self.adv_sel = 0;
                self.step_sel = 0;
                self.screen = ScreenKind::HelpAdvice;
            }
            Ok(a) => {
                self.screen = self.help_return;
                self.message = Some(format!(
                    "Claude had no concrete proposal. Cause: {}",
                    a.cause
                ));
            }
            Err(e) => {
                self.screen = self.help_return;
                self.message = Some(format!("Ask Claude: {e}"));
            }
        }
    }

    pub fn on_patch_result(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.last_patch = self.pending_patch.take();
                self.screen = self.help_return;
                self.message = Some("Patch applied. r retry · u undo the patch.".into());
            }
            Err(e) => {
                self.pending_patch = None;
                self.message = Some(format!("Patch not applied: {e}"));
            }
        }
    }

    pub fn on_patch_reverted(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.last_patch = None;
                self.message = Some("Patch undone.".into());
            }
            Err(e) => self.message = Some(format!("Could not undo the patch: {e}")),
        }
    }

    pub fn on_copied(&mut self, result: Result<(), String>) {
        self.message = Some(match result {
            Ok(()) => "Copied to the clipboard.".into(),
            Err(e) => format!("Could not copy: {e}"),
        });
    }

    fn key_help_advice(&mut self, key: KeyEvent, ctrl_c: bool) -> Action {
        let n = self.advice.as_ref().map(|a| a.proposals.len()).unwrap_or(0);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = self.help_return,
            _ if ctrl_c => self.screen = self.help_return,
            KeyCode::Down | KeyCode::Char('j') if self.adv_sel + 1 < n => self.adv_sel += 1,
            KeyCode::Up | KeyCode::Char('k') => self.adv_sel = self.adv_sel.saturating_sub(1),
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < n => {
                self.adv_sel = c as usize - '1' as usize;
                self.step_sel = 0;
                self.screen = ScreenKind::HelpDetail;
            }
            KeyCode::Enter if n > 0 => {
                self.step_sel = 0;
                self.screen = ScreenKind::HelpDetail;
            }
            _ => {}
        }
        Action::None
    }

    fn key_help_detail(&mut self, key: KeyEvent, ctrl_c: bool) -> Action {
        let Some(proposal) = self
            .advice
            .as_ref()
            .and_then(|a| a.proposals.get(self.adv_sel))
            .cloned()
        else {
            self.screen = ScreenKind::HelpAdvice;
            return Action::None;
        };
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) || ctrl_c {
            self.screen = ScreenKind::HelpAdvice;
            return Action::None;
        }
        match proposal {
            Proposal::Patch { diff, .. } => match key.code {
                KeyCode::Char('a') => {
                    self.pending_patch = Some(diff.clone());
                    Action::ApplyPatch { diff }
                }
                _ => Action::None,
            },
            Proposal::Steps { steps, .. } => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') if self.step_sel + 1 < steps.len() => {
                        self.step_sel += 1;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.step_sel = self.step_sel.saturating_sub(1);
                    }
                    KeyCode::Char('c') => {
                        if let Some(text) = steps.get(self.step_sel).and_then(|s| s.command.clone())
                        {
                            return Action::Copy { text };
                        }
                    }
                    KeyCode::Enter => {
                        let run = steps.get(self.step_sel).and_then(|s| s.run.clone());
                        if let Some(idx) =
                            run.and_then(|id| self.project.commands.iter().position(|c| c.id == id))
                        {
                            self.password = None;
                            self.open_command(idx);
                        } else {
                            self.message = Some("This step has no command dost can open.".into());
                        }
                    }
                    _ => {}
                }
                Action::None
            }
        }
    }

    pub(crate) fn run_failed(&self) -> bool {
        self.error.is_some() || self.last_exit != Some(0) || self.state.any_failed()
    }

    fn retry(&mut self, mode: RunMode) -> Action {
        self.message = if self.command().needs_become && self.password.is_none() {
            Some("The previous sudo password was not accepted. Enter it again.".into())
        } else {
            None
        };
        self.begin(mode)
    }

    /// Retry and type the sudo password again (a no-op for commands without one).
    fn retry_with_new_password(&mut self, mode: RunMode) -> Action {
        if !self.command().needs_become {
            return Action::None;
        }
        self.password = None;
        self.message = Some("Enter the sudo password again.".into());
        self.begin(mode)
    }

    pub(crate) fn hints(&self) -> Vec<&crate::catalog::ErrorRule> {
        hints_for(&self.project.errors, &self.state, &self.last_tail)
    }

    fn open_command(&mut self, idx: usize) {
        self.cur = Some(idx);
        self.typed.clear();
        self.message = None;
        self.screen = ScreenKind::Preflight;
    }

    fn back_to_menu(&mut self) {
        self.screen = ScreenKind::Menu;
        self.message = None;
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
            focus: self.command().focus.clone(),
            frame: self.state.frame,
            ..RunState::default()
        };
        self.running_mode = mode;
        self.message = None;
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
            ScreenKind::HelpConfirm => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.help_consent = true;
                    self.send_help()
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                    self.screen = self.help_return;
                    Action::None
                }
                _ if ctrl_c => {
                    self.screen = self.help_return;
                    Action::None
                }
                _ => Action::None,
            },
            ScreenKind::HelpAsking => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => Action::CancelHelp,
                _ if ctrl_c => Action::CancelHelp,
                _ => Action::None,
            },
            ScreenKind::HelpAdvice => self.key_help_advice(key, ctrl_c),
            ScreenKind::HelpDetail => self.key_help_detail(key, ctrl_c),
            ScreenKind::CheckFailed => match key.code {
                KeyCode::Char('h') => self.start_help(),
                KeyCode::Char('u') => self.undo_patch(),
                KeyCode::Char('r') => self.retry(RunMode::Check),
                KeyCode::Char('p') => self.retry_with_new_password(RunMode::Check),
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
                KeyCode::Char('h') if self.run_failed() => self.start_help(),
                KeyCode::Char('u') => self.undo_patch(),
                KeyCode::Char('r') if self.run_failed() => self.retry(RunMode::Apply),
                KeyCode::Char('p') if self.run_failed() => {
                    self.retry_with_new_password(RunMode::Apply)
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
                let pw = BecomePassword::new(bytes);
                self.pw_len = 0;
                if !pw.is_ansible_safe() {
                    self.message = Some(
                        "Ansible strips spaces around the password: leading or trailing spaces are not supported. Try again."
                            .into(),
                    );
                    return Action::None;
                }
                self.message = None;
                self.password = Some(pw);
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

    /// Advance the animation by one frame (called on every UI tick).
    pub fn tick(&mut self) {
        self.state.frame = self.state.frame.wrapping_add(1);
    }

    pub fn run_focus(&self) -> Vec<String> {
        self.state.focus.clone()
    }

    pub fn run_frame(&self) -> u64 {
        self.state.frame
    }

    pub fn run_finished(&self) -> bool {
        self.state.finished
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
        self.state.finished = true;
        self.last_exit = outcome.exit_code;
        self.last_tail = outcome.output_tail;
        let cmd_id = self.command().id.clone();
        self.store.append_history(&HistoryRecord::from_run(
            &cmd_id,
            &self.state,
            outcome.exit_code,
        ));
        let success = outcome.exit_code == Some(0) && !self.state.any_failed();
        if !success && self.password_rejected() {
            // A password that sudo refused must not be reused by the next attempt.
            self.password = None;
            self.message =
                Some("The sudo password was not accepted. Press r to retry with a new one.".into());
        }
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
