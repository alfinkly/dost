//! End-to-end tests of the binary without a terminal, using a fake ansible-playbook.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const STREAM: &str = r#"echo '{"t":"start","mode":"'"$DOST_MODE"'"}'
echo '{"t":"task","name":"install nginx"}'
echo '{"t":"result","host":"web1","task":"install nginx","changed":true,"failed":false,"skipped":false,"unreachable":false,"msg":null,"diff":null}'
echo '{"t":"recap","host":"web1","ok":1,"changed":1,"failed":0,"unreachable":0}'"#;

fn project(fake_body: &str) -> (TempDir, TempDir) {
    let proj = TempDir::new().unwrap();
    let root = proj.path();
    std::fs::create_dir_all(root.join("playbooks")).unwrap();
    std::fs::create_dir_all(root.join(".dost/catalog")).unwrap();
    std::fs::write(root.join("playbooks/base.yml"), "- hosts: all\n").unwrap();
    std::fs::write(
        root.join(".dost/catalog/base.yml"),
        "id: base\ntitle: Base\nplaybook: playbooks/base.yml\nabout: Base setup\ndone: \"All good on {{ hosts | join(', ') }}\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".dost/errors.yml"),
        "- match: \"Permission denied\"\n  cause: Wrong SSH key\n  fix: Add your key to the agent\n",
    )
    .unwrap();
    let bin = TempDir::new().unwrap();
    let script = bin.path().join("ansible-playbook");
    std::fs::write(
        &script,
        format!("#!/bin/sh\n[ \"$1\" = \"--version\" ] && {{ echo 'ansible-playbook [core 2.21.0]'; exit 0; }}\n{fake_body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (proj, bin)
}

fn dost(root: &Path, bin: &Path, args: &[&str]) -> Output {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    Command::new(env!("CARGO_BIN_EXE_dost"))
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .output()
        .unwrap()
}

#[test]
fn non_tty_check_prints_plain_steps_and_hint_to_apply() {
    let (proj, bin) = project(STREAM);
    let out = dost(proj.path(), bin.path(), &["run", "base"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("web1") && text.contains("install nginx"),
        "{text}"
    );
    assert!(text.contains("Check passed"), "{text}");
    assert!(text.contains("--apply"), "{text}");
    assert!(
        !text.contains('\u{1b}'),
        "plain output must have no escape codes: {text:?}"
    );
}

#[test]
fn non_tty_apply_prints_done_message_and_records_history() {
    let (proj, bin) = project(STREAM);
    let out = dost(proj.path(), bin.path(), &["run", "base", "--apply"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("All good on web1"), "{text}");
    let hist = dost(proj.path(), bin.path(), &["history"]);
    let h = String::from_utf8_lossy(&hist.stdout);
    assert!(h.contains("base") && h.contains("apply"), "{h}");
}

#[test]
fn non_tty_failed_check_prints_hint_and_fails() {
    let (proj, bin) = project(
        r#"echo '{"t":"start","mode":"check"}'
echo '{"t":"result","host":"web1","task":"ping","changed":false,"failed":true,"skipped":false,"unreachable":true,"msg":"Permission denied (publickey)","diff":null}'
exit 4"#,
    );
    let out = dost(proj.path(), bin.path(), &["run", "base", "--apply"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success());
    assert!(
        text.contains("Wrong SSH key") && text.contains("Add your key to the agent"),
        "{text}"
    );
    assert!(
        !text.contains("All good"),
        "must not apply after a failed check:\n{text}"
    );
}

#[test]
fn status_shows_chain_progress() {
    let (proj, bin) = project(STREAM);
    std::fs::create_dir_all(proj.path().join(".dost/catalog/chains")).unwrap();
    std::fs::write(
        proj.path().join(".dost/catalog/chains/node.yml"),
        "id: node\ntitle: Node\nsteps: [base]\n",
    )
    .unwrap();
    let before =
        String::from_utf8_lossy(&dost(proj.path(), bin.path(), &["status", "node"]).stdout)
            .to_string();
    assert!(
        before.contains("0/1") && before.contains("todo"),
        "{before}"
    );
    dost(proj.path(), bin.path(), &["run", "base", "--apply"]);
    let after = String::from_utf8_lossy(&dost(proj.path(), bin.path(), &["status", "node"]).stdout)
        .to_string();
    assert!(after.contains("1/1") && after.contains("done"), "{after}");
}

#[test]
fn missing_project_gives_clear_message() {
    let empty = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let out = dost(empty.path(), bin.path(), &["run", "base"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("dost init"));
}

#[test]
fn run_without_ansible_gives_a_clear_message() {
    let (proj, _bin) = project(STREAM);
    let empty = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dost"))
        .args(["run", "base"])
        .current_dir(proj.path())
        .env("PATH", empty.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("ansible-playbook was not found"), "{err}");
}

#[test]
fn run_with_old_ansible_gives_a_clear_message() {
    let (proj, bin) = project(STREAM);
    std::fs::write(
        bin.path().join("ansible-playbook"),
        "#!/bin/sh\necho 'ansible-playbook [core 2.12.1]'\n",
    )
    .unwrap();
    let out = dost(proj.path(), bin.path(), &["run", "base"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("too old"), "{err}");
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn sigint_in_plain_mode_stops_the_ansible_child() {
    use std::process::Stdio;
    let pidfile = TempDir::new().unwrap();
    let pf = pidfile.path().join("pid");
    let (proj, bin) = project(&format!("echo $$ > {}\nsleep 30", pf.display()));
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_dost"))
        .args(["run", "base", "--apply"])
        .current_dir(proj.path())
        .env("PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pid = String::new();
    for _ in 0..100 {
        if let Ok(t) = std::fs::read_to_string(&pf) {
            if !t.trim().is_empty() {
                pid = t.trim().to_string();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(!pid.is_empty(), "fake ansible never started");
    Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    let mut dead = false;
    for _ in 0..80 {
        if !alive(&pid) {
            dead = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !dead {
        let _ = Command::new("kill").args(["-KILL", &pid]).status();
    }
    assert!(
        dead,
        "ansible child {pid} kept running after dost got SIGINT"
    );
}
