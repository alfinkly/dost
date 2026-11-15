use std::process::Command;

fn dost() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dost"))
}

#[test]
fn help_lists_all_subcommands() {
    let out = dost().arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for sub in [
        "run", "chain", "status", "history", "init", "demo", "doctor",
    ] {
        assert!(
            text.contains(sub),
            "help is missing subcommand {sub}:\n{text}"
        );
    }
}

#[test]
fn doctor_reports_missing_ansible() {
    let empty = std::env::temp_dir().join("dost-empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let out = dost().arg("doctor").env("PATH", &empty).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(err.contains("ansible"), "stderr: {err}");
}
