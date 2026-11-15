//! Runs real ansible-playbook against localhost. Enabled with DOST_ANSIBLE_TESTS=1.
use dost::stream::{parse_line, RunMode, RunState, CALLBACK_PLUGIN};
use std::process::Command;

fn run(mode: &str) -> RunState {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("cb");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("dost_stream.py"), CALLBACK_PLUGIN).unwrap();
    let pb = dir.path().join("pb.yml");
    std::fs::write(
        &pb,
        r#"- hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: ports
      ansible.builtin.debug:
        msg: { dost: { v: 1, kind: ports, state: { "22": "on", "80": "off" } } }
    - name: links
      ansible.builtin.debug:
        msg: { dost: { v: 1, kind: link, key: "{{ item }}", state: up } }
      loop: [a-b, b-c]
    - name: hidden
      no_log: true
      ansible.builtin.debug:
        msg: { dost: { v: 1, kind: note, text: secret } }
"#,
    )
    .unwrap();
    let mut cmd = Command::new("ansible-playbook");
    cmd.arg(&pb)
        .env("ANSIBLE_CALLBACK_PLUGINS", &plugins)
        .env("ANSIBLE_STDOUT_CALLBACK", "dost_stream")
        .env("DOST_MODE", mode)
        .env("ANSIBLE_LOCALHOST_WARNING", "False")
        .env("ANSIBLE_INVENTORY_UNPARSED_WARNING", "False");
    if mode == "check" {
        cmd.arg("--check");
    }
    let out = cmd.output().expect("ansible-playbook must be installed");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut st = RunState::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(l) = parse_line(line) {
            st.apply(l);
        }
    }
    st
}

fn enabled() -> bool {
    std::env::var("DOST_ANSIBLE_TESTS").as_deref() == Ok("1")
}

#[test]
fn events_arrive_in_apply_mode() {
    if !enabled() {
        return;
    }
    let st = run("apply");
    assert_eq!(st.mode, RunMode::Apply);
    let k = ("localhost".to_string(), "ports".to_string(), String::new());
    assert_eq!(st.events[&k]["state"]["80"], "off");
    let a = ("localhost".into(), "link".into(), "a-b".into());
    let c = ("localhost".into(), "link".into(), "b-c".into());
    assert!(st.events.contains_key(&a) && st.events.contains_key(&c));
}

#[test]
fn no_log_task_emits_no_event_and_check_mode_is_marked() {
    if !enabled() {
        return;
    }
    let st = run("check");
    assert_eq!(st.mode, RunMode::Check);
    assert!(!st.events.keys().any(|(_, kind, _)| kind == "note"));
    assert!(st.recap.contains_key("localhost"));
}

#[test]
fn plays_and_loop_items_arrive_with_labels() {
    if !enabled() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("cb");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("dost_stream.py"), CALLBACK_PLUGIN).unwrap();
    let pb = dir.path().join("pb.yml");
    std::fs::write(
        &pb,
        r#"- name: First play
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Make accounts
      ansible.builtin.command: "true"
      changed_when: item == 'alice'
      loop: [alice, bob]
      loop_control: { label: "{{ item }}" }
    - name: Plain task
      ansible.builtin.command: "true"
      changed_when: false
- name: Second play
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Secret loop
      no_log: true
      ansible.builtin.command: "true"
      loop: [x]
"#,
    )
    .unwrap();
    let out = Command::new("ansible-playbook")
        .arg(&pb)
        .env("ANSIBLE_CALLBACK_PLUGINS", &plugins)
        .env("ANSIBLE_STDOUT_CALLBACK", "dost_stream")
        .env("DOST_MODE", "apply")
        .env("ANSIBLE_LOCALHOST_WARNING", "False")
        .env("ANSIBLE_INVENTORY_UNPARSED_WARNING", "False")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut st = RunState::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(l) = parse_line(line) {
            st.apply(l);
        }
    }
    assert_eq!(
        st.plays,
        vec!["First play".to_string(), "Second play".to_string()]
    );
    let alice = st
        .items
        .iter()
        .find(|i| i.item == "alice")
        .expect("alice item");
    assert!(alice.changed && alice.play == "First play" && alice.task == "Make accounts");
    let bob = st.items.iter().find(|i| i.item == "bob").expect("bob item");
    assert!(!bob.changed);
    let secret = st
        .items
        .iter()
        .find(|i| i.task == "Secret loop")
        .expect("secret item");
    assert!(secret.item.is_empty(), "no_log loop items carry no label");
    assert_eq!(
        st.results
            .iter()
            .find(|r| r.task == "Plain task")
            .unwrap()
            .play,
        "First play"
    );
}
