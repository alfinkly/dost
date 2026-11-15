//! `dost init`: create catalog stubs from the playbooks of a project.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Default)]
pub struct InitReport {
    pub created: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

#[derive(Serialize)]
struct Stub {
    id: String,
    title: String,
    playbook: String,
    about: String,
    scene: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    watch: Vec<String>,
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("yml") | Some("yaml")
        ) {
            out.push(p);
        }
    }
}

fn sanitize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn title_of(stem: &str) -> String {
    let words = stem.replace(['-', '_'], " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => words,
    }
}

/// Roles named in a `roles:` list that exist under `roles/`.
fn roles_used(root: &Path, text: &str) -> Vec<String> {
    let item_name = Regex::new(r"(?:role|name):\s*([\w.\-]+)").unwrap();
    let mut roles = Vec::new();
    let mut in_roles = false;
    let mut roles_indent = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("roles:") {
            in_roles = true;
            roles_indent = indent;
            continue;
        }
        if !in_roles || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let name = item_name
                .captures(rest)
                .map(|c| c[1].to_string())
                .unwrap_or_else(|| rest.trim().trim_matches(['"', '\'']).to_string());
            if !name.is_empty() && !roles.contains(&name) {
                roles.push(name);
            }
        } else if indent <= roles_indent {
            in_roles = false;
        }
    }
    roles
        .into_iter()
        .filter(|r| root.join("roles").join(r).is_dir())
        .collect()
}

fn write_new(path: &Path, content: &str, report: &mut InitReport) -> Result<()> {
    if path.exists() {
        report.skipped.push(path.to_path_buf());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    report.created.push(path.to_path_buf());
    Ok(())
}

pub fn init(root: &Path) -> Result<InitReport> {
    let pb_dir = root.join("playbooks");
    if !pb_dir.is_dir() {
        bail!(
            "no playbooks/ directory in {}; dost init scans playbooks/**/*.yml",
            root.display()
        );
    }
    let mut files = Vec::new();
    collect(&pb_dir, &mut files);
    let mut report = InitReport::default();
    let mut used_ids: Vec<String> = Vec::new();
    for file in files {
        let rel = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("playbook");
        let mut id = sanitize(stem);
        if used_ids.contains(&id) {
            let parent = file
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("x");
            id = format!("{}-{id}", sanitize(parent));
        }
        used_ids.push(id.clone());
        let text = fs::read_to_string(&file).unwrap_or_default();
        let watch = roles_used(root, &text)
            .into_iter()
            .map(|r| format!("roles/{r}/**"))
            .collect();
        let stub = Stub {
            id: id.clone(),
            title: title_of(stem),
            playbook: rel.to_string_lossy().replace('\\', "/"),
            about: String::new(),
            scene: "chain".into(),
            watch,
        };
        let yaml = serde_yaml_ng::to_string(&stub)?;
        write_new(
            &root.join(".dost/catalog").join(format!("{id}.yml")),
            &yaml,
            &mut report,
        )?;
    }
    write_new(&root.join(".dost/errors.yml"), "[]\n", &mut report)?;
    write_new(
        &root.join(".dost/.gitignore"),
        "state/\nhistory/\n",
        &mut report,
    )?;
    Ok(report)
}
