use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

pub const MIN_ANSIBLE: Version = Version::new(2, 15, 0);

#[derive(Debug, Clone)]
pub struct AnsibleInfo {
    pub version: Version,
    pub path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error("ansible-playbook was not found in PATH; install ansible-core {MIN_ANSIBLE} or newer")]
    NotFound,
    #[error("could not read the ansible version from: {0}")]
    Unparsable(String),
    #[error("ansible-core {found} is too old; dost needs {MIN_ANSIBLE} or newer")]
    TooOld { found: Version },
}

/// Parse the first line of `ansible --version`, e.g. `ansible [core 2.21.4]`.
pub fn parse_version(first_line: &str) -> Option<Version> {
    let rest = first_line.split("core ").nth(1)?;
    let raw: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    Version::parse(&raw).ok()
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

pub fn check_ansible() -> Result<AnsibleInfo, DoctorError> {
    let path = find_in_path("ansible-playbook").ok_or(DoctorError::NotFound)?;
    let out = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|_| DoctorError::NotFound)?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let first = text.lines().next().unwrap_or("").to_string();
    let version = parse_version(&first).ok_or(DoctorError::Unparsable(first))?;
    if version < MIN_ANSIBLE {
        return Err(DoctorError::TooOld { found: version });
    }
    Ok(AnsibleInfo { version, path })
}

/// Every problem found in a project: catalog errors, scene errors, event tasks with `no_log`.
pub fn validate_project(root: &Path) -> Vec<String> {
    let project = match crate::catalog::load_project(root) {
        Ok(p) => p,
        Err(e) => return e.problems().iter().map(|p| p.to_string()).collect(),
    };
    let mut out = Vec::new();
    match crate::scenes::load_scenes(&project) {
        Err(e) => out.push(e.to_string()),
        Ok(scenes) => {
            for cmd in &project.commands {
                if let Some(id) = &cmd.scene {
                    if !scenes.contains_key(id) {
                        out.push(format!(
                            "command `{}`: scene `{id}` does not exist (built-in: chain, ports, vpn, config)",
                            cmd.id
                        ));
                    }
                }
            }
        }
    }
    for cmd in &project.commands {
        let strings = cmd
            .extra_vars
            .iter()
            .chain(cmd.apply_extra_vars.iter())
            .chain(cmd.inventory.iter())
            .chain(cmd.limit.iter());
        for s in strings {
            if s.contains("FILL_IN") {
                out.push(format!(
                    "command `{}`: `{s}` still contains a FILL_IN placeholder; set the real value in .dost/catalog",
                    cmd.id
                ));
            }
        }
        let file = root.join(&cmd.playbook);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(doc) = serde_yaml_ng::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        warn_no_log_events(&doc, &cmd.playbook, &mut out);
    }
    out
}

fn mentions_dost(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(m) => m.contains_key("dost") || m.values().any(mentions_dost),
        serde_json::Value::Array(a) => a.iter().any(mentions_dost),
        _ => false,
    }
}

fn warn_no_log_events(v: &serde_json::Value, file: &Path, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(m) => {
            let no_log = matches!(m.get("no_log"), Some(serde_json::Value::Bool(true)))
                || matches!(
                    m.get("no_log").and_then(|x| x.as_str()),
                    Some("true" | "yes")
                );
            if no_log && mentions_dost(v) {
                let name = m
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("(unnamed)");
                out.push(format!(
                    "{}: task `{name}` has no_log: true and emits a dost event; the event will be dropped",
                    file.display()
                ));
            }
            for x in m.values() {
                warn_no_log_events(x, file, out);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| warn_no_log_events(x, file, out)),
        _ => {}
    }
}

/// Check that Ansible can load the bundled `dost_stream` callback.
pub fn check_plugin_loads() -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    std::fs::write(
        dir.path().join("dost_stream.py"),
        crate::stream::CALLBACK_PLUGIN,
    )
    .map_err(|e| e.to_string())?;
    let out = Command::new("ansible-doc")
        .args(["-t", "callback", "dost_stream"])
        .env("ANSIBLE_CALLBACK_PLUGINS", dir.path())
        .output()
        .map_err(|e| format!("ansible-doc not runnable: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_core_version() {
        assert_eq!(
            parse_version("ansible [core 2.21.4]"),
            Some(Version::new(2, 21, 4))
        );
    }

    #[test]
    fn rejects_unknown_format() {
        assert_eq!(parse_version("something else"), None);
    }
}
