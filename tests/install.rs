//! The installer puts a working `dost` into PREFIX/bin and can remove it again.
use std::process::Command;
use tempfile::TempDir;

fn script() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}

fn run(prefix: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new("sh")
        .arg(script())
        .args(args)
        .env("PREFIX", prefix.path())
        .output()
        .expect("sh must be available")
}

#[test]
fn install_puts_a_working_dost_in_prefix_bin_and_uninstall_removes_it() {
    let prefix = TempDir::new().unwrap();
    let out = run(&prefix, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = prefix.path().join("bin/dost");
    assert!(bin.is_file(), "binary missing at {}", bin.display());
    let v = Command::new(&bin).arg("--version").output().unwrap();
    assert!(
        String::from_utf8_lossy(&v.stdout).starts_with("dost "),
        "{v:?}"
    );

    let out = run(&prefix, &["--uninstall"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!bin.exists());
}

#[test]
fn install_warns_when_bin_is_not_on_path() {
    let prefix = TempDir::new().unwrap();
    let out = run(&prefix, &[]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("PATH"),
        "a prefix outside PATH must produce a hint:\n{text}"
    );
}

#[test]
fn install_refuses_unknown_arguments() {
    let prefix = TempDir::new().unwrap();
    let out = run(&prefix, &["--frobnicate"]);
    assert!(!out.status.success());
}
