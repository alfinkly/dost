use dost::catalog::{Command, Danger};
use dost::runner::{build_plan, display_command, run, RunOutcome};
use dost::secret::BecomePassword;
use dost::stream::RunMode;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn cmd() -> Command {
    Command {
        id: "perimeter".into(),
        title: "Perimeter".into(),
        playbook: PathBuf::from("playbooks/perimeter.yml"),
        about: String::new(),
        danger: Danger::Low,
        needs_become: true,
        scene: None,
        after: vec![],
        done: None,
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

fn args(plan: &dost::runner::RunPlan) -> Vec<String> {
    plan.argv
        .iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect()
}

#[test]
fn check_plan_has_check_and_diff_flags() {
    let plan = build_plan(Path::new("/p"), &cmd(), RunMode::Check, Path::new("/cb"));
    let a = args(&plan);
    assert!(a.contains(&"--check".to_string()) && a.contains(&"--diff".to_string()));
    assert_eq!(a[0], "ansible-playbook");
    assert!(plan.env.contains(&("DOST_MODE".into(), "check".into())));
    assert!(plan
        .env
        .contains(&("ANSIBLE_STDOUT_CALLBACK".into(), "dost_stream".into())));
    assert_eq!(plan.cwd, PathBuf::from("/p"));
}

#[test]
fn apply_plan_has_neither() {
    let plan = build_plan(Path::new("/p"), &cmd(), RunMode::Apply, Path::new("/cb"));
    let a = args(&plan);
    assert!(!a.contains(&"--check".to_string()) && !a.contains(&"--diff".to_string()));
    assert!(plan.env.contains(&("DOST_MODE".into(), "apply".into())));
}

#[test]
fn catalog_fields_map_to_flags() {
    let mut c = cmd();
    c.inventory = Some("inventory/hosts.yml".into());
    c.limit = Some("web1".into());
    c.tags = vec!["a".into(), "b".into()];
    c.extra_vars = vec!["x=1".into(), "y=2".into()];
    let a = args(&build_plan(
        Path::new("/p"),
        &c,
        RunMode::Apply,
        Path::new("/cb"),
    ));
    let joined = a.join(" ");
    assert!(joined.contains("-i inventory/hosts.yml"), "{joined}");
    assert!(joined.contains("--limit web1"), "{joined}");
    assert!(joined.contains("--tags a,b"), "{joined}");
    assert!(
        joined.contains("-e x=1") && joined.contains("-e y=2"),
        "{joined}"
    );
}

#[test]
fn password_not_in_argv_env_or_display() {
    let secret = "p@ss w\"ord$1";
    let pw = BecomePassword::new(secret.as_bytes().to_vec());
    let plan = build_plan(Path::new("/p"), &cmd(), RunMode::Apply, Path::new("/cb"));
    let all = format!("{:?} {:?} {}", plan.argv, plan.env, display_command(&plan));
    assert!(!all.contains("p@ss"));
    assert!(!format!("{pw:?}").contains("p@ss"));
}

fn fake_ansible(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-ansible-playbook");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[tokio::test]
async fn password_reaches_child_intact() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("got");
    let script = fake_ansible(
        dir.path(),
        r#"while [ $# -gt 0 ]; do case "$1" in --become-password-file) f="$2"; shift;; esac; shift; done
cat "$f" > "$OUT_FILE""#,
    );
    let mut plan = build_plan(dir.path(), &cmd(), RunMode::Apply, Path::new("/cb"));
    plan.argv[0] = script.into();
    plan.env
        .push(("OUT_FILE".into(), out.to_string_lossy().into()));
    let secret = "p@ss w\"ord$1 ünï".as_bytes().to_vec();
    let (tx, _rx) = mpsc::channel(16);
    let outcome = run(
        plan,
        Some(BecomePassword::new(secret.clone())),
        tx,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.exit_code, Some(0), "{}", outcome.output_tail);
    assert_eq!(std::fs::read(&out).unwrap(), secret);
}

#[tokio::test]
async fn cancel_kills_child() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_ansible(dir.path(), "sleep 30");
    let mut plan = build_plan(dir.path(), &cmd(), RunMode::Apply, Path::new("/cb"));
    plan.argv[0] = script.into();
    let (tx, _rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        c2.cancel();
    });
    let started = std::time::Instant::now();
    let outcome: RunOutcome = run(plan, None, tx, cancel).await.unwrap();
    assert!(outcome.cancelled);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[tokio::test]
async fn stream_lines_and_stderr_are_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_ansible(
        dir.path(),
        r#"echo '{"t":"start","mode":"apply"}'
echo 'plain text before callback' 
echo 'boom' >&2
exit 2"#,
    );
    let mut plan = build_plan(dir.path(), &cmd(), RunMode::Apply, Path::new("/cb"));
    plan.argv[0] = script.into();
    let (tx, mut rx) = mpsc::channel(16);
    let outcome = run(plan, None, tx, CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.exit_code, Some(2));
    assert!(outcome.output_tail.contains("boom"));
    assert!(outcome.output_tail.contains("plain text before callback"));
    assert!(rx.recv().await.is_some());
}

#[tokio::test]
async fn non_utf8_and_huge_lines_do_not_stall_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let script = fake_ansible(
        dir.path(),
        r#"printf '\377\376 not utf8\n'
head -c 3000000 /dev/zero | tr '\0' a; echo
echo '{"t":"start","mode":"apply"}'
head -c 3000000 /dev/zero | tr '\0' b; echo
echo '{"t":"task","name":"after"}'"#,
    );
    let mut plan = build_plan(dir.path(), &cmd(), RunMode::Apply, Path::new("/cb"));
    plan.argv[0] = script.into();
    let (tx, mut rx) = mpsc::channel(64);
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        run(plan, None, tx, CancellationToken::new()),
    )
    .await
    .expect("run must not stall on bad output")
    .unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert!(
        outcome.output_tail.len() < 1_000_000,
        "tail must be bounded: {}",
        outcome.output_tail.len()
    );
    let mut names = vec![];
    while let Ok(l) = rx.try_recv() {
        if let dost::stream::StreamLine::TaskStart { name } = l {
            names.push(name);
        }
    }
    assert_eq!(
        names,
        vec!["after".to_string()],
        "lines after the bad ones must still arrive"
    );
}

#[test]
fn only_passwords_without_edge_whitespace_are_safe_for_ansible() {
    assert!(BecomePassword::new(b"p@ss w\"ord$1 \xc3\xbc".to_vec()).is_ansible_safe());
    for bad in [&b" lead"[..], b"trail ", b"nl\n", b"\ttab", b""] {
        assert!(
            !BecomePassword::new(bad.to_vec()).is_ansible_safe(),
            "{bad:?}"
        );
    }
}

#[test]
fn apply_only_vars_are_added_for_apply_and_not_for_check() {
    let mut c = cmd();
    c.extra_vars = vec!["always=1".into()];
    c.apply_extra_vars = vec!["confirm=yes".into()];
    let check = args(&build_plan(
        Path::new("/p"),
        &c,
        RunMode::Check,
        Path::new("/cb"),
    ))
    .join(" ");
    let apply = args(&build_plan(
        Path::new("/p"),
        &c,
        RunMode::Apply,
        Path::new("/cb"),
    ))
    .join(" ");
    assert!(
        check.contains("always=1") && !check.contains("confirm=yes"),
        "{check}"
    );
    assert!(
        apply.contains("always=1") && apply.contains("confirm=yes"),
        "{apply}"
    );
}

#[tokio::test]
async fn progress_file_lines_arrive_as_events_while_the_child_runs() {
    let dir = tempfile::tempdir().unwrap();
    let go = dir.path().join("go");
    let script = fake_ansible(
        dir.path(),
        r#"echo '{"dost":{"v":1,"kind":"transfer","key":"files","state":"running","done":5,"total":10}}' >> "$DOST_PROGRESS_FILE"
while [ ! -f "$GO_FILE" ]; do sleep 0.05; done
echo '{"dost":{"v":1,"kind":"transfer","key":"files","state":"done","done":10,"total":10}}' >> "$DOST_PROGRESS_FILE""#,
    );
    let mut plan = build_plan(dir.path(), &cmd(), RunMode::Apply, dir.path());
    assert!(
        plan.env.iter().any(|(k, _)| k == "DOST_PROGRESS_FILE"),
        "the plan must name a progress file"
    );
    plan.argv[0] = script.into();
    plan.env
        .push(("GO_FILE".into(), go.to_string_lossy().into()));
    let (tx, mut rx) = mpsc::channel(64);
    let handle = tokio::spawn(run(plan, None, tx, CancellationToken::new()));
    // The child waits for `go`, so this holds however loaded the machine is.
    let first = tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv())
        .await
        .expect("the first progress line must arrive while the child is still running")
        .unwrap();
    assert!(
        matches!(first, dost::stream::StreamLine::Event { ref kind, .. } if kind == "transfer"),
        "{first:?}"
    );
    assert!(!handle.is_finished(), "the child is still waiting for `go`");
    std::fs::write(&go, "").unwrap();
    let outcome = handle.await.unwrap().unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    let mut states = vec![];
    while let Ok(l) = rx.try_recv() {
        if let dost::stream::StreamLine::Event { data, .. } = l {
            states.push(data["state"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(
        states,
        vec!["done".to_string()],
        "the last line is drained after the child exits"
    );
}
