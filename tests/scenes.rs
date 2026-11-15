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

const REPORT: &str = r#"
id: nodeup
use: report
bind:
  stages:
    - { play: "^Node baseline", label: Base }
    - { play: "^Admin users", label: Users }
    - { play: "^k3s", label: Cluster }
  sections:
    - id: users
      title: Users
      play: "^Admin users"
      rules:
        - { task: "^Admin accounts$", label: "{item}", changed: created, ok: already there, failed: not created }
    - id: network
      title: Network
      rules:
        - { task: "^Ping every peer over wg1$", label: "{item}", ok: reachable }
    - id: tools
      title: Tools
      rules:
        - { task: "^Install Docker$", label: Docker, changed: installed, ok: already installed }
        - { task: "^Enable ufw", label: ufw, changed: enabled, ok: already enabled }
"#;

fn report_scene() -> Scene {
    let def: SceneDef = serde_yaml_ng::from_str(REPORT).unwrap();
    load_scenes(&project(vec![def]))
        .unwrap()
        .remove("nodeup")
        .unwrap()
}

fn item(host: &str, task: &str, it: &str, changed: bool, failed: bool) -> StreamLine {
    StreamLine::Item {
        host: host.into(),
        task: task.into(),
        item: it.into(),
        changed,
        failed,
        skipped: false,
    }
}

fn draw_report(scene: &Scene, st: &RunState) -> String {
    let area = Rect::new(0, 0, 100, 24);
    let mut buf = Buffer::empty(area);
    scene.render(st, area, &mut buf);
    text(&buf)
}

#[test]
fn report_scene_tells_created_from_already_there_and_failed() {
    let scene = report_scene();
    let mut st = RunState::default();
    st.apply(StreamLine::Play {
        name: "Admin users".into(),
    });
    st.apply(item("n4", "Admin accounts", "alice", true, false));
    st.apply(item("n4", "Admin accounts", "bob", false, false));
    st.apply(item("n4", "Admin accounts", "carol", false, true));
    let t = draw_report(&scene, &st);
    let row = |n: &str| t.lines().find(|l| l.contains(n)).unwrap_or("").to_string();
    assert!(row("alice").contains("created"), "{t}");
    assert!(row("bob").contains("already there"), "{t}");
    assert!(
        row("carol").contains("not created") && row("carol").contains('✗'),
        "{t}"
    );
    assert!(t.contains("Users"), "{t}");
}

#[test]
fn report_scene_shows_peers_and_tools_from_task_results() {
    let scene = report_scene();
    let mut st = RunState::default();
    st.apply(item(
        "n4",
        "Ping every peer over wg1",
        "circle-ops",
        false,
        false,
    ));
    st.apply(item(
        "n4",
        "Ping every peer over wg1",
        "opkit-hero",
        false,
        false,
    ));
    st.apply(result("n4", "Install Docker", true, false));
    st.apply(result("n4", "Enable ufw with default deny", false, false));
    let t = draw_report(&scene, &st);
    assert!(
        t.contains("circle-ops") && t.contains("opkit-hero") && t.contains("reachable"),
        "{t}"
    );
    assert!(
        t.lines()
            .any(|l| l.contains("Docker") && l.contains("installed")),
        "{t}"
    );
    assert!(
        t.lines()
            .any(|l| l.contains("ufw") && l.contains("already enabled")),
        "{t}"
    );
}

#[test]
fn report_scene_stage_header_shows_done_running_and_pending_stages() {
    let scene = report_scene();
    let mut st = RunState::default();
    st.apply(StreamLine::Play {
        name: "Node baseline".into(),
    });
    st.apply(result("n4", "Install Docker", true, false));
    st.apply(StreamLine::Play {
        name: "Admin users".into(),
    });
    let t = draw_report(&scene, &st);
    let head = t.lines().next().unwrap();
    assert!(head.contains("✓ Base"), "{head}");
    assert!(
        head.contains("Users") && !head.contains("✓ Users") && !head.contains("○ Users"),
        "running stage spins: {head}"
    );
    assert!(head.contains("○ Cluster"), "{head}");
    st.apply(StreamLine::Recap {
        host: "n4".into(),
        ok: 1,
        changed: 1,
        failed: 0,
        unreachable: 0,
    });
    assert!(draw_report(&scene, &st)
        .lines()
        .next()
        .unwrap()
        .contains("✓ Users"));
}

#[test]
fn report_scene_focus_limits_hosts_and_many_hosts_get_a_prefix() {
    let scene = report_scene();
    let mut st = RunState {
        focus: vec!["n4".into()],
        ..Default::default()
    };
    st.apply(item(
        "n4",
        "Ping every peer over wg1",
        "circle-ops",
        false,
        false,
    ));
    st.apply(item(
        "other",
        "Ping every peer over wg1",
        "n4",
        false,
        false,
    ));
    let t = draw_report(&scene, &st);
    assert!(
        t.contains("circle-ops") && !t.contains("other"),
        "focus hides other hosts:\n{t}"
    );
    let mut all = RunState::default();
    all.apply(item(
        "n4",
        "Ping every peer over wg1",
        "circle-ops",
        false,
        false,
    ));
    all.apply(item(
        "other",
        "Ping every peer over wg1",
        "n4",
        false,
        false,
    ));
    let t = draw_report(&scene, &all);
    assert!(
        t.contains("n4: circle-ops") && t.contains("other: n4"),
        "{t}"
    );
}

#[test]
fn report_scene_rejects_bad_definitions_at_load() {
    let none: SceneDef = serde_yaml_ng::from_str("id: r\nuse: report\n").unwrap();
    assert!(Scene::from_def(&none).is_err());
    let bad_re: SceneDef = serde_yaml_ng::from_str(
        "id: r\nuse: report\nbind:\n  sections:\n    - { id: a, title: A, rules: [ { task: '(open' } ] }\n",
    )
    .unwrap();
    assert!(Scene::from_def(&bad_re).is_err());
}

fn transfer_scene() -> Scene {
    let def: SceneDef =
        serde_yaml_ng::from_str("id: tr\nuse: transfer\nbind:\n  titles: [PROD, PREPROD]\n")
            .unwrap();
    load_scenes(&project(vec![def]))
        .unwrap()
        .remove("tr")
        .unwrap()
}

fn tr(key: &str, state: &str, done: Option<u64>, total: Option<u64>) -> StreamLine {
    event(
        "laptop",
        "transfer",
        Some(key),
        json!({"label": key, "from": format!("src/{key}"), "to": format!("dst/{key}"), "state": state, "done": done, "total": total}),
    )
}

#[test]
fn transfer_scene_lists_rows_in_arrival_order_with_titles_and_progress() {
    let scene = transfer_scene();
    let mut st = RunState::default();
    st.apply(tr(
        "zeta",
        "running",
        Some(5 * 1024 * 1024),
        Some(10 * 1024 * 1024),
    ));
    st.apply(tr("alpha", "done", None, None));
    let t = draw(&scene, &st);
    let lines: Vec<&str> = t.lines().collect();
    assert!(
        lines[0].contains("PROD") && lines[0].contains("PREPROD"),
        "{t}"
    );
    let zeta = lines.iter().position(|l| l.contains("zeta")).unwrap();
    let alpha = lines.iter().position(|l| l.contains("alpha")).unwrap();
    assert!(
        zeta < alpha,
        "rows keep arrival order, not alphabetical:\n{t}"
    );
    assert!(
        lines[zeta].contains("5.0 MB")
            && lines[zeta].contains("10.0 MB")
            && lines[zeta].contains("src/zeta")
            && lines[zeta].contains("dst/zeta"),
        "{t}"
    );
}

#[test]
fn transfer_scene_maps_states_to_glyphs_and_rejects_nothing_without_titles() {
    let scene = transfer_scene();
    let mut st = RunState::default();
    st.apply(tr("ok", "done", None, None));
    st.apply(tr("bad", "missing", None, None));
    st.apply(tr("wait", "pending", None, None));
    let t = draw(&scene, &st);
    assert!(t.lines().find(|l| l.contains("ok")).unwrap().contains('✓'));
    assert!(t.lines().find(|l| l.contains("bad")).unwrap().contains('✗'));
    assert!(t
        .lines()
        .find(|l| l.contains("wait"))
        .unwrap()
        .contains('○'));
    let plain: SceneDef = serde_yaml_ng::from_str("id: p\nuse: transfer\n").unwrap();
    assert!(Scene::from_def(&plain).is_ok(), "titles are optional");
}

#[test]
fn transfer_scene_strips_prefixes_from_from_and_to() {
    let def: SceneDef = serde_yaml_ng::from_str(
        "id: s\nuse: transfer\nbind:\n  titles: [PROD, PREPROD]\n  strip: [\"kv/opkit/\"]\n",
    )
    .unwrap();
    let scene = load_scenes(&project(vec![def]))
        .unwrap()
        .remove("s")
        .unwrap();
    let mut st = RunState::default();
    st.apply(event(
        "laptop",
        "transfer",
        Some("backend/TOKEN"),
        json!({"from": "kv/opkit/prod/backend", "to": "kv/opkit/preprod/backend", "state": "done"}),
    ));
    let t = draw(&scene, &st);
    let row = t.lines().find(|l| l.contains("backend/TOKEN")).unwrap();
    assert!(
        row.contains("prod/backend") && row.contains("preprod/backend"),
        "{row}"
    );
    assert!(!row.contains("kv/opkit/"), "{row}");
}
