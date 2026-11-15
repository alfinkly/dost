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
