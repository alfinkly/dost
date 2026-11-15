use dost::catalog::{Project, SceneDef, Theme as CatTheme};
use dost::scenes::{load_scenes, parse_path, resolve_path, Scene};
use dost::stream::{RunMode, RunState, StreamLine};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;
use std::path::PathBuf;

fn text(buf: &Buffer) -> String {
    let a = buf.area;
    (0..a.height)
        .map(|y| {
            (0..a.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw(scene: &Scene, st: &RunState) -> String {
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    scene.render(st, area, &mut buf);
    text(&buf)
}

fn project(scenes: Vec<SceneDef>) -> Project {
    Project {
        root: PathBuf::from("."),
        commands: vec![],
        chains: vec![],
        errors: vec![],
        scenes,
        theme: CatTheme::default(),
    }
}

fn result(host: &str, task: &str, changed: bool, failed: bool) -> StreamLine {
    StreamLine::TaskResult {
        host: host.into(),
        task: task.into(),
        changed,
        failed,
        skipped: false,
        unreachable: false,
        module: None,
        msg: None,
        diff: None,
    }
}

fn event(host: &str, kind: &str, key: Option<&str>, data: serde_json::Value) -> StreamLine {
    StreamLine::Event {
        host: host.into(),
        kind: kind.into(),
        key: key.map(String::from),
        data,
    }
}

#[test]
fn resolve_path_dotted_and_wildcard() {
    let root = json!({"event": {"link": {"x": {"state": "up"}, "y": {"state": "down"}}, "ports": {"state": {"22": "on"}}}});
    assert_eq!(
        resolve_path(&root, "event.ports.state").unwrap(),
        vec![json!({"22": "on"})]
    );
    let states = resolve_path(&root, "event.link[*].state").unwrap();
    assert_eq!(states, vec![json!("up"), json!("down")]);
    assert!(resolve_path(&root, "event.nothing.here")
        .unwrap()
        .is_empty());
    assert!(parse_path("event..x").is_err());
    assert!(parse_path("event.[x").is_err());
}

#[test]
fn bad_path_is_error_at_load() {
    let def: SceneDef =
        serde_yaml_ng::from_str("id: x\nuse: table\nbind: { rows: 'event..ports' }\n").unwrap();
    assert!(Scene::from_def(&def).is_err());
    let unknown: SceneDef = serde_yaml_ng::from_str("id: y\nuse: hologram\n").unwrap();
    assert!(Scene::from_def(&unknown).is_err());
}

#[test]
fn ports_scene_shows_was_and_now_columns() {
    let scenes = load_scenes(&project(vec![])).unwrap();
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    st.apply(event(
        "web1",
        "ports",
        None,
        json!({"state": {"22": "on", "80": "off"}, "was": {"22": "on", "80": "on"}}),
    ));
    let t = draw(&scenes["ports"], &st);
    for s in ["web1", "port", "was", "now", "80", "off"] {
        assert!(t.contains(s), "missing {s}:\n{t}");
    }
}

#[test]
fn vpn_scene_marks_unlinked_node() {
    let scenes = load_scenes(&project(vec![])).unwrap();
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    st.apply(result("ops", "wg", true, false));
    st.apply(result("hero", "wg", true, false));
    st.apply(event(
        "ops",
        "link",
        Some("ops<>hero"),
        json!({"a": "ops", "b": "hero", "state": "up"}),
    ));
    st.apply(event(
        "hero",
        "link",
        Some("hero<>mail"),
        json!({"a": "hero", "b": "mail", "state": "down"}),
    ));
    let t = draw(&scenes["vpn"], &st);
    assert!(t.contains("═"), "{t}");
    assert!(t.contains("✗"), "{t}");
    assert!(
        t.contains("mail"),
        "node that never joined must still be visible:\n{t}"
    );
}

#[test]
fn config_scene_lists_changed_tasks_without_diff() {
    let scenes = load_scenes(&project(vec![])).unwrap();
    let mut st = RunState::default();
    st.apply(result("web1", "install nginx", true, false));
    st.apply(result("web1", "check config", false, false));
    let t = draw(&scenes["config"], &st);
    assert!(t.contains("install nginx"), "{t}");
    assert!(!t.contains("check config"), "{t}");
}

#[test]
fn check_mode_scene_has_preview_label() {
    let scenes = load_scenes(&project(vec![])).unwrap();
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Check,
    });
    st.apply(result("web1", "install nginx", true, false));
    let t = draw(&scenes["config"], &st);
    assert!(t.contains("ПРОВЕРКА, предварительно"), "{t}");
    let mut apply = RunState::default();
    apply.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    assert!(!draw(&scenes["config"], &apply).contains("ПРОВЕРКА"));
}

#[test]
fn user_scene_overrides_builtin() {
    let user: SceneDef = serde_yaml_ng::from_str("id: ports\nuse: results\n").unwrap();
    let scenes = load_scenes(&project(vec![user])).unwrap();
    let mut st = RunState::default();
    st.apply(result("web1", "install nginx", true, false));
    assert!(draw(&scenes["ports"], &st).contains("install nginx"));
}

#[test]
fn chain_scene_shows_task_status() {
    let scenes = load_scenes(&project(vec![])).unwrap();
    let mut st = RunState::default();
    st.apply(StreamLine::TaskStart { name: "one".into() });
    st.apply(result("h", "one", false, false));
    st.apply(StreamLine::TaskStart { name: "two".into() });
    st.apply(result("h", "two", false, true));
    let t = draw(&scenes["chain"], &st);
    assert!(t.contains("✓ one") && t.contains("✗ two"), "{t}");
}
