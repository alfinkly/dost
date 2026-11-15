use dost::catalog::load_project;
use dost::demo;
use dost::doctor::validate_project;
use dost::init::init;
use dost::scenes::load_scenes;
use std::fs;
use tempfile::TempDir;

fn playbooks() -> TempDir {
    let d = TempDir::new().unwrap();
    let r = d.path();
    fs::create_dir_all(r.join("playbooks/sub")).unwrap();
    fs::create_dir_all(r.join("roles/web/tasks")).unwrap();
    fs::write(r.join("roles/web/tasks/main.yml"), "- debug: msg=a\n").unwrap();
    fs::write(
        r.join("playbooks/a.yml"),
        "- hosts: all\n  roles:\n    - web\n    - role: missing\n",
    )
    .unwrap();
    fs::write(r.join("playbooks/sub/b.yml"), "- hosts: all\n  tasks: []\n").unwrap();
    d
}

#[test]
fn init_creates_stub_per_playbook() {
    let d = playbooks();
    let report = init(d.path()).unwrap();
    assert_eq!(
        report
            .created
            .iter()
            .filter(|p| p.to_string_lossy().contains("catalog"))
            .count(),
        2
    );
    let p = load_project(d.path()).unwrap();
    let ids: Vec<&str> = p.commands.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"a") && ids.contains(&"b"), "{ids:?}");
    let a = p.commands.iter().find(|c| c.id == "a").unwrap();
    assert_eq!(
        a.watch,
        vec!["roles/web/**".to_string()],
        "only existing roles are watched"
    );
}

#[test]
fn init_does_not_overwrite() {
    let d = playbooks();
    init(d.path()).unwrap();
    let file = d.path().join(".dost/catalog/a.yml");
    fs::write(
        &file,
        "id: a\ntitle: Mine\nplaybook: playbooks/a.yml\nabout: custom\n",
    )
    .unwrap();
    let report = init(d.path()).unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains("custom"));
    assert!(report.skipped.iter().any(|p| p.ends_with("a.yml")));
}

#[test]
fn init_without_playbooks_dir_is_a_clear_error() {
    let d = TempDir::new().unwrap();
    let err = init(d.path()).unwrap_err().to_string();
    assert!(err.contains("playbooks"), "{err}");
}

#[test]
fn demo_extracts_runnable_project() {
    let d = TempDir::new().unwrap();
    demo::extract(d.path()).unwrap();
    let p = load_project(d.path()).unwrap();
    assert!(p.commands.len() >= 3 && p.chains.len() == 1);
    load_scenes(&p).unwrap();
    assert!(validate_project(d.path()).is_empty());
}

#[test]
fn doctor_lists_all_catalog_problems() {
    let d = TempDir::new().unwrap();
    fs::create_dir_all(d.path().join(".dost/catalog")).unwrap();
    fs::write(
        d.path().join(".dost/catalog/x.yml"),
        "id: x\ntitle: X\nplaybook: playbooks/none.yml\nafter: [ghost]\n",
    )
    .unwrap();
    let problems = validate_project(d.path());
    assert!(
        problems.iter().any(|p| p.contains("playbook")),
        "{problems:?}"
    );
    assert!(problems.iter().any(|p| p.contains("after")), "{problems:?}");
}

#[test]
fn doctor_warns_about_no_log_on_event_task() {
    let d = TempDir::new().unwrap();
    fs::create_dir_all(d.path().join(".dost/catalog")).unwrap();
    fs::create_dir_all(d.path().join("playbooks")).unwrap();
    fs::write(
        d.path().join("playbooks/p.yml"),
        "- hosts: all\n  tasks:\n    - name: hidden\n      no_log: true\n      ansible.builtin.debug:\n        msg: { dost: { v: 1, kind: note } }\n",
    )
    .unwrap();
    fs::write(
        d.path().join(".dost/catalog/p.yml"),
        "id: p\ntitle: P\nplaybook: playbooks/p.yml\n",
    )
    .unwrap();
    let problems = validate_project(d.path());
    assert!(
        problems.iter().any(|p| p.contains("no_log")),
        "{problems:?}"
    );
}

#[test]
fn demo_runs_with_real_ansible_when_enabled() {
    if std::env::var("DOST_ANSIBLE_TESTS").as_deref() != Ok("1") {
        return;
    }
    let d = TempDir::new().unwrap();
    demo::extract(d.path()).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_dost"))
        .args(["run", "vpn", "--apply"])
        .current_dir(d.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("The tunnel was built on"), "{text}");
    assert!(text.contains("event") && text.contains("link"), "{text}");
}

#[test]
fn doctor_reports_command_pointing_at_unknown_scene() {
    let d = TempDir::new().unwrap();
    fs::create_dir_all(d.path().join(".dost/catalog")).unwrap();
    fs::create_dir_all(d.path().join("playbooks")).unwrap();
    fs::write(d.path().join("playbooks/p.yml"), "- hosts: all\n").unwrap();
    fs::write(
        d.path().join(".dost/catalog/p.yml"),
        "id: p\ntitle: P\nplaybook: playbooks/p.yml\nscene: nonexistent\n",
    )
    .unwrap();
    let problems = validate_project(d.path());
    assert!(
        problems.iter().any(|p| p.contains("nonexistent")),
        "{problems:?}"
    );
}

#[test]
fn doctor_reports_fill_in_placeholders() {
    let d = TempDir::new().unwrap();
    fs::create_dir_all(d.path().join(".dost/catalog")).unwrap();
    fs::create_dir_all(d.path().join("playbooks")).unwrap();
    fs::write(d.path().join("playbooks/p.yml"), "- hosts: all\n").unwrap();
    fs::write(
        d.path().join(".dost/catalog/p.yml"),
        "id: p\ntitle: P\nplaybook: playbooks/p.yml\nextra_vars: [\"node_public_ip=FILL_IN_NODE_IP\"]\n",
    )
    .unwrap();
    let problems = validate_project(d.path());
    assert!(
        problems.iter().any(|p| p.contains("FILL_IN")),
        "{problems:?}"
    );
}
