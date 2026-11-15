use std::path::PathBuf;
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
    #[error("ansible was not found in PATH; install ansible-core {MIN_ANSIBLE} or newer")]
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
    let path = find_in_path("ansible").ok_or(DoctorError::NotFound)?;
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
