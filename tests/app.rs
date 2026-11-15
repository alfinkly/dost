use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dost::app::{Action, App, ScreenKind};
use dost::catalog::{Chain, Command, Danger, ErrorRule, Project, Theme as CatTheme};
use dost::runner::RunOutcome;
use dost::store::{playbook_hash, Store};
use dost::stream::{RunMode, StreamLine};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use regex::Regex;
use std::path::PathBuf;
use tempfile::TempDir;

fn cmd(id: &str, about: &str, danger: Danger, become_: bool) -> Command {
    Command {
        id: id.into(),
        title: id.to_uppercase(),
        playbook: PathBuf::from(format!("playbooks/{id}.yml")),
        about: about.into(),
        danger,
        needs_become: become_,
        scene: None,
        after: vec![],
        done: Some("All good on {{ hosts | join(', ') }}".into()),
        inventory: None,
        limit: None,
        tags: vec![],
        extra_vars: vec![],
        apply_extra_vars: vec![],
        focus: vec![],
        check: true,
        watch: vec![],
    }
}

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("playbooks")).unwrap();
        std::fs::create_dir_all(dir.path().join(".dost")).unwrap();
        for id in ["base", "perimeter"] {
            std::fs::write(
                dir.path().join(format!("playbooks/{id}.yml")),
                "- hosts: all\n",
            )
            .unwrap();
        }
        Fixture { dir }
    }

    fn project(&self) -> Project {
        Project {
            root: self.dir.path().to_path_buf(),
            commands: vec![
                cmd("base", "Base setup of a node", Danger::Low, false),
                cmd(
                    "perimeter",
                    "Closes every port except SSH",
                    Danger::High,
                    true,
                ),
            ],
            chains: vec![Chain {
                id: "node".into(),
                title: "Node".into(),
                steps: vec!["base".into(), "perimeter".into()],
            }],
            errors: vec![ErrorRule {
                match_: Regex::new("Too many authentication failures").unwrap(),
                module: None,
                cause: "Provider limits new SSH connections".into(),
                fix: "Enable ControlMaster".into(),
                link: None,
            }],
            scenes: vec![],
            theme: CatTheme::default(),
        }
    }

    fn app(&self) -> App {
        App::new(self.project(), Store::open(self.dir.path()))
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}
fn code(k: KeyCode) -> KeyEvent {
    KeyEvent::new(k, KeyModifiers::NONE)
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(c));
    }
}

fn screen(app: &App) -> String {
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ok(code: i32) -> RunOutcome {
    RunOutcome {
        exit_code: Some(code),
        cancelled: false,
        output_tail: String::new(),
    }
}

fn result(host: &str, failed: bool, msg: Option<&str>) -> StreamLine {
    StreamLine::TaskResult {
        host: host.into(),
        task: "t".into(),
        changed: !failed,
        failed,
        skipped: false,
        unreachable: false,
        module: None,
        msg: msg.map(String::from),
        diff: None,
    }
}

#[test]
fn menu_shows_about_and_chain_progress() {
    let fx = Fixture::new();
    let t = screen(&fx.app());
    assert!(
        t.contains("BASE") && t.contains("Base setup of a node"),
        "{t}"
    );
    assert!(t.contains("node") && t.contains("0/2"), "{t}");

    let c = fx.project().commands[0].clone();
    Store::open(fx.dir.path()).mark_done("node", "base", &playbook_hash(&c, fx.dir.path()));
    let t = screen(&fx.app());
    assert!(t.contains("1/2"), "{t}");
}

#[test]
fn high_danger_requires_typed_id() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down)); // perimeter (commands are listed after chains)
    app.handle_key(code(KeyCode::Down));
    assert!(matches!(app.handle_key(code(KeyCode::Enter)), Action::None));
    assert_eq!(app.screen_kind(), ScreenKind::Preflight);
    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.screen_kind(), ScreenKind::Password);
    type_str(&mut app, "pw");
    let act = app.handle_key(code(KeyCode::Enter));
    assert!(matches!(
        act,
        Action::Start {
            mode: RunMode::Check,
            ..
        }
    ));
    app.on_stream(StreamLine::RunStart {
        mode: RunMode::Check,
    });
    app.on_stream(result("h", false, None));
    app.on_finished(ok(0));
    assert_eq!(app.screen_kind(), ScreenKind::ApplyConfirm);
    assert!(
        matches!(app.handle_key(code(KeyCode::Enter)), Action::None),
        "no id typed yet"
    );
    type_str(&mut app, "wrong");
    assert!(matches!(app.handle_key(code(KeyCode::Enter)), Action::None));
    for _ in 0..5 {
        app.handle_key(code(KeyCode::Backspace));
    }
    type_str(&mut app, "perimeter");
    let act = app.handle_key(code(KeyCode::Enter));
    assert!(
        matches!(
            act,
            Action::Start {
                mode: RunMode::Apply,
                ..
            }
        ),
        "{act:?}"
    );
    assert!(
        app.password().is_some(),
        "password is reused, not asked twice"
    );
}

#[test]
fn check_failure_blocks_apply_until_confirmed() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down)); // base
    app.handle_key(code(KeyCode::Enter));
    let act = app.handle_key(code(KeyCode::Enter));
    assert!(matches!(
        act,
        Action::Start {
            mode: RunMode::Check,
            ..
        }
    ));
    app.on_stream(StreamLine::RunStart {
        mode: RunMode::Check,
    });
    app.on_stream(result("h", true, Some("boom")));
    app.on_finished(ok(2));
    assert_eq!(app.screen_kind(), ScreenKind::CheckFailed);
    app.handle_key(key('a'));
    assert_eq!(app.screen_kind(), ScreenKind::ApplyConfirm);
    assert!(
        matches!(app.handle_key(code(KeyCode::Enter)), Action::None),
        "forced apply needs the id"
    );
    type_str(&mut app, "base");
    assert!(matches!(
        app.handle_key(code(KeyCode::Enter)),
        Action::Start {
            mode: RunMode::Apply,
            ..
        }
    ));
}

#[test]
fn enter_after_failed_check_goes_back_to_menu_not_apply() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(result("h", true, None));
    app.on_finished(ok(2));
    assert!(matches!(app.handle_key(code(KeyCode::Enter)), Action::None));
    assert_eq!(app.screen_kind(), ScreenKind::Menu);
}

#[test]
fn password_field_never_renders_characters() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    type_str(&mut app, "hunter2");
    let t = screen(&app);
    assert!(!t.contains("hunter2"), "{t}");
    assert!(t.contains("•••••••"), "{t}");
}

#[test]
fn failure_shows_matching_hint() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(result(
        "h",
        true,
        Some("UNREACHABLE: Too many authentication failures"),
    ));
    app.on_finished(ok(4));
    let t = screen(&app);
    assert!(t.contains("Provider limits new SSH connections"), "{t}");
    assert!(t.contains("Enable ControlMaster"), "{t}");
}

#[test]
fn quit_while_running_cancels_and_returns_to_menu() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    assert_eq!(app.screen_kind(), ScreenKind::Running);
    assert!(matches!(app.handle_key(key('q')), Action::Cancel));
    app.on_finished(RunOutcome {
        exit_code: None,
        cancelled: true,
        output_tail: String::new(),
    });
    assert_eq!(app.screen_kind(), ScreenKind::Menu);
    assert!(app.password().is_none());
}

#[test]
fn successful_apply_shows_done_message_and_marks_chain() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter)); // check starts
    app.on_stream(StreamLine::RunStart {
        mode: RunMode::Check,
    });
    app.on_stream(result("web1", false, None));
    app.on_finished(ok(0));
    assert!(matches!(
        app.handle_key(code(KeyCode::Enter)),
        Action::Start {
            mode: RunMode::Apply,
            ..
        }
    ));
    app.on_stream(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    app.on_stream(result("web1", false, None));
    app.on_finished(ok(0));
    assert_eq!(app.screen_kind(), ScreenKind::Result);
    let t = screen(&app);
    assert!(t.contains("All good on web1"), "{t}");
    app.handle_key(code(KeyCode::Enter));
    let t = screen(&app);
    assert!(t.contains("1/2"), "chain progress must advance:\n{t}");
}

fn result_with_module(host: &str, module: &str, msg: &str) -> StreamLine {
    StreamLine::TaskResult {
        host: host.into(),
        task: "t".into(),
        changed: false,
        failed: true,
        skipped: false,
        unreachable: false,
        module: Some(module.into()),
        msg: Some(msg.into()),
        diff: None,
    }
}

fn project_with_module_rule(fx: &Fixture) -> Project {
    let mut p = fx.project();
    p.errors = vec![ErrorRule {
        match_: Regex::new("refused").unwrap(),
        module: Some("ansible.builtin.ping".into()),
        cause: "Ping specific cause".into(),
        fix: "Ping specific fix".into(),
        link: None,
    }];
    p
}

fn failed_check_screen(project: Project, fx: &Fixture, module: &str) -> String {
    let mut app = App::new(project, Store::open(fx.dir.path()));
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(result_with_module("h", module, "connection refused"));
    app.on_finished(ok(2));
    screen(&app)
}

#[test]
fn rule_with_module_matches_the_failing_module() {
    let fx = Fixture::new();
    let hit = failed_check_screen(project_with_module_rule(&fx), &fx, "ansible.builtin.ping");
    assert!(hit.contains("Ping specific cause"), "{hit}");
    let miss = failed_check_screen(project_with_module_rule(&fx), &fx, "ansible.builtin.copy");
    assert!(!miss.contains("Ping specific cause"), "{miss}");
}

#[test]
fn bad_user_scene_keeps_builtin_scenes_and_says_so() {
    let fx = Fixture::new();
    let mut p = fx.project();
    p.scenes = vec![serde_yaml_ng::from_str("id: broken\nuse: hologram\n").unwrap()];
    let mut app = App::new(p, Store::open(fx.dir.path()));
    let menu = screen(&app);
    assert!(
        menu.contains("broken"),
        "the scene problem must be visible:\n{menu}"
    );
    // built-in scenes still render during a run
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(StreamLine::TaskStart { name: "one".into() });
    app.on_stream(result("h", false, None));
    assert!(screen(&app).contains("one"), "chain scene must still draw");
}

#[test]
fn password_with_edge_whitespace_is_refused() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    type_str(&mut app, " pw ");
    let act = app.handle_key(code(KeyCode::Enter));
    assert!(matches!(act, Action::None), "{act:?}");
    assert_eq!(app.screen_kind(), ScreenKind::Password);
    assert!(app.password().is_none());
    assert!(screen(&app).contains("spaces"), "{}", screen(&app));
    type_str(&mut app, "pw");
    assert!(matches!(
        app.handle_key(code(KeyCode::Enter)),
        Action::Start { .. }
    ));
}

#[test]
fn focus_of_the_command_reaches_the_run_state_and_tick_advances_the_frame() {
    let fx = Fixture::new();
    let mut p = fx.project();
    p.commands[0].focus = vec!["bao".into()];
    let mut app = App::new(p, Store::open(fx.dir.path()));
    app.handle_key(code(KeyCode::Down)); // base
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter)); // start check
    assert_eq!(app.run_focus(), vec!["bao".to_string()]);
    let f0 = app.run_frame();
    app.tick();
    app.tick();
    assert_eq!(app.run_frame(), f0 + 2);
    app.on_finished(ok(0));
    assert!(app.run_finished(), "a finished run stops the spinner");
}

fn event_line(kind: &str, key: &str, state: &str, detail: &str) -> StreamLine {
    StreamLine::Event {
        host: "h".into(),
        kind: kind.into(),
        key: Some(key.into()),
        data: serde_json::json!({"state": state, "detail": detail}),
    }
}

#[test]
fn a_red_link_fails_the_check_even_when_ansible_exits_zero() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter)); // check starts
    app.on_stream(result("h", false, None));
    app.on_stream(event_line("step", "ports", "failed", "8003 ✗"));
    app.on_finished(ok(0));
    assert_eq!(
        app.screen_kind(),
        ScreenKind::CheckFailed,
        "exit 0 with a red link is not a pass"
    );
    let t = screen(&app);
    assert!(
        t.contains("ports") && t.contains("8003 ✗"),
        "the red link is listed:\n{t}"
    );
}

#[test]
fn a_red_link_after_apply_shows_failure_and_does_not_advance_the_chain() {
    let fx = Fixture::new();
    let mut app = fx.app();
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(result("h", false, None));
    app.on_finished(ok(0));
    assert!(matches!(
        app.handle_key(code(KeyCode::Enter)),
        Action::Start {
            mode: RunMode::Apply,
            ..
        }
    ));
    app.on_stream(result("h", false, None));
    app.on_stream(event_line("step", "gpu", "failed", "no ssh session"));
    app.on_finished(ok(0));
    let t = screen(&app);
    assert!(
        t.contains("The run failed") && t.contains("no ssh session"),
        "{t}"
    );
    assert!(!t.contains("All good"), "{t}");
    app.handle_key(code(KeyCode::Enter));
    assert!(
        screen(&app).contains("0/2"),
        "the chain must not advance after a red link"
    );
}

#[test]
fn error_rules_also_match_the_details_of_red_links() {
    let fx = Fixture::new();
    let mut p = fx.project();
    p.errors[0].match_ = Regex::new("no ssh session").unwrap();
    let mut app = App::new(p, Store::open(fx.dir.path()));
    app.handle_key(code(KeyCode::Down));
    app.handle_key(code(KeyCode::Enter));
    app.handle_key(code(KeyCode::Enter));
    app.on_stream(event_line(
        "step",
        "gpu",
        "failed",
        "no ssh session to the GPU server",
    ));
    app.on_finished(ok(0));
    assert!(screen(&app).contains("Provider limits new SSH connections"));
}
