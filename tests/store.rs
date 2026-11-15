use dost::catalog::{Chain, Command, Danger};
use dost::store::{playbook_hash, HistoryRecord, StepStatus, Store};
use dost::stream::{RunMode, RunState, StreamLine};
use std::path::PathBuf;
use tempfile::TempDir;

fn cmd(watch: Vec<&str>) -> Command {
    Command {
        id: "base".into(),
        title: "Base".into(),
        playbook: PathBuf::from("playbooks/base.yml"),
        about: String::new(),
        danger: Danger::Low,
        needs_become: false,
        scene: None,
        after: vec![],
        done: None,
        inventory: None,
        limit: None,
        tags: vec![],
        extra_vars: vec![],
        check: true,
        watch: watch.into_iter().map(String::from).collect(),
    }
}

fn root() -> TempDir {
    let d = TempDir::new().unwrap();
    std::fs::create_dir_all(d.path().join("playbooks")).unwrap();
    std::fs::create_dir_all(d.path().join("roles/web/tasks")).unwrap();
    std::fs::create_dir_all(d.path().join(".dost")).unwrap();
    std::fs::write(d.path().join("playbooks/base.yml"), "- hosts: all\n").unwrap();
    std::fs::write(
        d.path().join("roles/web/tasks/main.yml"),
        "- debug: msg=a\n",
    )
    .unwrap();
    d
}

fn chain() -> Chain {
    Chain {
        id: "node".into(),
        title: "Node".into(),
        steps: vec!["base".into(), "wg".into()],
    }
}

#[test]
fn fresh_chain_is_all_todo() {
    let d = root();
    let store = Store::open(d.path());
    let p = store.chain_progress(&chain(), |_| "h".into());
    assert!(p.iter().all(|(_, s)| *s == StepStatus::Todo));
    assert_eq!(p.len(), 2);
}

#[test]
fn done_step_turns_maybe_stale_after_playbook_edit() {
    let d = root();
    let store = Store::open(d.path());
    let c = cmd(vec![]);
    let h1 = playbook_hash(&c, d.path());
    store.mark_done("node", "base", &h1);
    let now = |_: &str| playbook_hash(&c, d.path());
    assert_eq!(store.chain_progress(&chain(), now)[0].1, StepStatus::Done);
    std::fs::write(d.path().join("playbooks/base.yml"), "- hosts: web\n").unwrap();
    let now = |_: &str| playbook_hash(&c, d.path());
    assert_eq!(
        store.chain_progress(&chain(), now)[0].1,
        StepStatus::MaybeStale
    );
}

#[test]
fn watch_glob_change_marks_stale() {
    let d = root();
    let c = cmd(vec!["roles/web/**/*.yml"]);
    let before = playbook_hash(&c, d.path());
    std::fs::write(
        d.path().join("roles/web/tasks/main.yml"),
        "- debug: msg=b\n",
    )
    .unwrap();
    assert_ne!(before, playbook_hash(&c, d.path()));
    // without watch the role edit is invisible
    let plain = cmd(vec![]);
    let a = playbook_hash(&plain, d.path());
    std::fs::write(
        d.path().join("roles/web/tasks/main.yml"),
        "- debug: msg=c\n",
    )
    .unwrap();
    assert_eq!(a, playbook_hash(&plain, d.path()));
}

#[test]
fn history_record_has_no_password_or_no_log_values() {
    let d = root();
    let store = Store::open(d.path());
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    st.apply(StreamLine::TaskResult {
        host: "h".into(),
        task: "t".into(),
        changed: true,
        failed: false,
        skipped: false,
        unreachable: false,
        msg: Some("hunter2".into()),
        diff: Some("password=hunter2".into()),
    });
    let rec = HistoryRecord::from_run("base", &st, Some(0));
    store.append_history(&rec);
    let text = std::fs::read_to_string(d.path().join(".dost/history/runs.jsonl")).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("\"id\":\"base\""));
    assert_eq!(store.read_history().len(), 1);
}

#[test]
fn progress_survives_reopen() {
    let d = root();
    Store::open(d.path()).mark_done("node", "base", "abc");
    let again = Store::open(d.path());
    let p = again.chain_progress(&chain(), |_| "abc".into());
    assert_eq!(p[0].1, StepStatus::Done);
    assert_eq!(p[1].1, StepStatus::Todo);
}
