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

fn tree_def(extra: &str) -> SceneDef {
    serde_yaml_ng::from_str(&format!(
        "id: secrets\nuse: tree\nbind:\n  nodes:\n    - {{ id: bao, label: OpenBao }}\n    - {{ id: hero, label: opkit-hero, parent: bao }}\n    - {{ id: agent-stage, label: 'bao-agent@stage', parent: hero }}\n    - {{ id: agent-prod, label: 'bao-agent@prod', parent: hero }}\n{extra}"
    ))
    .unwrap()
}

fn tree_scene() -> Scene {
    load_scenes(&project(vec![tree_def("")]))
        .unwrap()
        .remove("secrets")
        .unwrap()
}

#[test]
fn tree_scene_spins_the_focus_node_while_running_and_checks_it_after() {
    let scene = tree_scene();
    let mut st = RunState {
        focus: vec!["bao".into()],
        ..Default::default()
    };
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    assert_eq!(
        st.focus,
        vec!["bao".to_string()],
        "focus survives the run start"
    );
    st.apply(result("circle-ops", "install", true, false));
    let running = draw(&scene, &st);
    assert!(
        running.contains("OpenBao") && running.contains("bao-agent@stage"),
        "{running}"
    );
    assert!(
        !running.contains('✓'),
        "a running node must not be checked yet:\n{running}"
    );
    st.apply(StreamLine::Recap {
        host: "circle-ops".into(),
        ok: 1,
        changed: 1,
        failed: 0,
        unreachable: 0,
    });
    let done = draw(&scene, &st);
    assert!(done.lines().next().unwrap().contains('✓'), "{done}");
}

#[test]
fn tree_scene_marks_failed_focus_node() {
    let scene = tree_scene();
    let mut st = RunState {
        focus: vec!["agent-prod".into()],
        ..Default::default()
    };
    st.apply(result("opkit-hero", "render", false, true));
    st.apply(StreamLine::Recap {
        host: "opkit-hero".into(),
        ok: 0,
        changed: 0,
        failed: 1,
        unreachable: 0,
    });
    let t = draw(&scene, &st);
    let line = t.lines().find(|l| l.contains("bao-agent@prod")).unwrap();
    assert!(line.contains('✗'), "{t}");
}

#[test]
fn tree_scene_event_status_overrides_and_unknown_nodes_are_dots() {
    let scene = tree_scene();
    let mut st = RunState::default();
    st.apply(result("opkit-hero", "t", false, false));
    st.apply(event(
        "opkit-hero",
        "node",
        Some("agent-stage"),
        json!({"state": "up"}),
    ));
    st.apply(event(
        "opkit-hero",
        "node",
        Some("agent-prod"),
        json!({"state": "down"}),
    ));
    let t = draw(&scene, &st);
    let stage = t.lines().find(|l| l.contains("bao-agent@stage")).unwrap();
    let prod = t.lines().find(|l| l.contains("bao-agent@prod")).unwrap();
    assert!(stage.contains('✓') && prod.contains('✗'), "{t}");
    let hero = t.lines().find(|l| l.contains("opkit-hero")).unwrap();
    assert!(
        hero.contains('·'),
        "no information about hero: a dot, not a tick:\n{t}"
    );
}

#[test]
fn tree_scene_rejects_bad_definitions_at_load() {
    let no_nodes: SceneDef = serde_yaml_ng::from_str("id: t\nuse: tree\n").unwrap();
    assert!(Scene::from_def(&no_nodes).is_err());
    let bad_parent: SceneDef = serde_yaml_ng::from_str(
        "id: t\nuse: tree\nbind:\n  nodes:\n    - { id: a, label: A, parent: ghost }\n",
    )
    .unwrap();
    assert!(Scene::from_def(&bad_parent).is_err());
    let dup: SceneDef = serde_yaml_ng::from_str(
        "id: t\nuse: tree\nbind:\n  nodes:\n    - { id: a, label: A }\n    - { id: a, label: B }\n",
    )
    .unwrap();
    assert!(Scene::from_def(&dup).is_err());
}

fn pipeline_scene() -> Scene {
    let def: SceneDef = serde_yaml_ng::from_str(
        "id: gpu\nuse: chain\nbind:\n  steps:\n    - { id: host, label: opkit-hero }\n    - { id: ports, label: Ports }\n    - { id: gpu, label: GPU host }\n    - { id: models, label: Models }\n",
    )
    .unwrap();
    load_scenes(&project(vec![def]))
        .unwrap()
        .remove("gpu")
        .unwrap()
}

fn step(host: &str, id: &str, state: &str, detail: Option<&str>) -> StreamLine {
    event(
        host,
        "step",
        Some(id),
        json!({"state": state, "detail": detail}),
    )
}

#[test]
fn pipeline_scene_walks_the_links_with_spinner_and_details() {
    let scene = pipeline_scene();
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    st.apply(step(
        "hero",
        "host",
        "ok",
        Some("gpu-tunnel.service active"),
    ));
    st.apply(step("hero", "ports", "running", None));
    let t = draw(&scene, &st);
    let line = |needle: &str| {
        t.lines()
            .find(|l| l.contains(needle))
            .unwrap_or("")
            .to_string()
    };
    assert!(
        line("opkit-hero").contains('✓')
            && line("opkit-hero").contains("gpu-tunnel.service active"),
        "{t}"
    );
    assert!(
        !line("Ports").contains('✓') && !line("Ports").contains('○'),
        "running link spins:\n{t}"
    );
    assert!(
        line("GPU host").contains('○') && line("Models").contains('○'),
        "not reached yet:\n{t}"
    );
    st.apply(step("hero", "ports", "ok", Some("8000 ✓  8002 ✓")));
    let t2 = draw(&scene, &st);
    assert!(
        t2.lines()
            .find(|l| l.contains("Ports"))
            .unwrap()
            .contains("8000 ✓  8002 ✓"),
        "{t2}"
    );
}

#[test]
fn pipeline_scene_marks_failed_link_and_an_interrupted_running_one() {
    let scene = pipeline_scene();
    let mut st = RunState::default();
    st.apply(step("hero", "host", "ok", None));
    st.apply(step("hero", "ports", "failed", Some("8003 ✗")));
    let t = draw(&scene, &st);
    assert!(
        t.lines()
            .find(|l| l.contains("Ports"))
            .unwrap()
            .contains('✗'),
        "{t}"
    );
    let mut st = RunState::default();
    st.apply(step("hero", "gpu", "running", None));
    st.apply(StreamLine::Recap {
        host: "hero".into(),
        ok: 1,
        changed: 0,
        failed: 0,
        unreachable: 0,
    });
    let t = draw(&scene, &st);
    assert!(
        t.lines()
            .find(|l| l.contains("GPU host"))
            .unwrap()
            .contains('✗'),
        "a link still running when the run ended did not finish:\n{t}"
    );
}

#[test]
fn pipeline_scene_rejects_bad_definitions_at_load() {
    let empty: SceneDef =
        serde_yaml_ng::from_str("id: g\nuse: chain\nbind:\n  steps: []\n").unwrap();
    assert!(Scene::from_def(&empty).is_err());
    let dup: SceneDef = serde_yaml_ng::from_str(
        "id: g\nuse: chain\nbind:\n  steps:\n    - { id: a, label: A }\n    - { id: a, label: B }\n",
    )
    .unwrap();
    assert!(Scene::from_def(&dup).is_err());
}
