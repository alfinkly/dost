//! Project catalog: commands, chains, known errors, scenes and theme.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Danger {
    #[default]
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub id: String,
    pub title: String,
    pub playbook: PathBuf,
    pub about: String,
    pub danger: Danger,
    pub needs_become: bool,
    pub scene: Option<String>,
    pub after: Vec<String>,
    pub done: Option<String>,
    pub inventory: Option<String>,
    pub limit: Option<String>,
    pub tags: Vec<String>,
    pub extra_vars: Vec<String>,
    pub check: bool,
    pub watch: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Chain {
    pub id: String,
    pub title: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ErrorRule {
    pub match_: Regex,
    pub module: Option<String>,
    pub cause: String,
    pub fix: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneDef {
    pub id: String,
    #[serde(rename = "use")]
    pub use_: String,
    #[serde(default)]
    pub bind: serde_json::Value,
    #[serde(default)]
    pub animation: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub commands: Vec<Command>,
    pub chains: Vec<Chain>,
    pub errors: Vec<ErrorRule>,
    pub scenes: Vec<SceneDef>,
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub file: PathBuf,
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.file.display(),
            self.field,
            self.message
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{}", .0.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("\n"))]
    Many(Vec<Problem>),
}

#[derive(Deserialize)]
struct RawCommand {
    id: String,
    title: String,
    playbook: PathBuf,
    #[serde(default)]
    about: String,
    #[serde(default)]
    danger: Danger,
    #[serde(default)]
    needs_become: bool,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default)]
    done: Option<String>,
    #[serde(default)]
    inventory: Option<String>,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    extra_vars: Vec<String>,
    #[serde(default = "yes")]
    check: bool,
    #[serde(default)]
    watch: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct RawChain {
    id: String,
    title: String,
    steps: Vec<String>,
}

#[derive(Deserialize)]
struct RawError {
    #[serde(rename = "match")]
    pattern: String,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    cause: String,
    #[serde(default)]
    fix: String,
    #[serde(default)]
    link: Option<String>,
}

/// Nearest ancestor of `start` (including itself) that contains a `.dost` directory.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".dost").is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("yml") | Some("yaml")
                )
        })
        .collect();
    v.sort();
    v
}

fn read_yaml<T: for<'de> Deserialize<'de>>(file: &Path, problems: &mut Vec<Problem>) -> Option<T> {
    let text = match fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            problems.push(Problem {
                file: file.to_path_buf(),
                field: "file".into(),
                message: e.to_string(),
            });
            return None;
        }
    };
    match serde_yaml_ng::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            problems.push(Problem {
                file: file.to_path_buf(),
                field: "yaml".into(),
                message: e.to_string(),
            });
            None
        }
    }
}

pub fn load_project(root: &Path) -> Result<Project, CatalogError> {
    let dost = root.join(".dost");
    let mut problems = Vec::new();
    let mut commands: Vec<(PathBuf, Command)> = Vec::new();

    for file in yaml_files(&dost.join("catalog")) {
        if let Some(raw) = read_yaml::<RawCommand>(&file, &mut problems) {
            let playbook = root.join(&raw.playbook);
            if !playbook.is_file() {
                problems.push(Problem {
                    file: file.clone(),
                    field: "playbook".into(),
                    message: format!("file {} does not exist", raw.playbook.display()),
                });
            }
            commands.push((
                file,
                Command {
                    id: raw.id,
                    title: raw.title,
                    playbook: raw.playbook,
                    about: raw.about,
                    danger: raw.danger,
                    needs_become: raw.needs_become,
                    scene: raw.scene,
                    after: raw.after,
                    done: raw.done,
                    inventory: raw.inventory,
                    limit: raw.limit,
                    tags: raw.tags,
                    extra_vars: raw.extra_vars,
                    check: raw.check,
                    watch: raw.watch,
                },
            ));
        }
    }
    let ids: BTreeSet<&str> = commands.iter().map(|(_, c)| c.id.as_str()).collect();
    for (file, c) in &commands {
        for a in &c.after {
            if !ids.contains(a.as_str()) {
                problems.push(Problem {
                    file: file.clone(),
                    field: "after".into(),
                    message: format!("unknown command id `{a}`"),
                });
            }
        }
    }

    let mut chains = Vec::new();
    for file in yaml_files(&dost.join("catalog").join("chains")) {
        if let Some(raw) = read_yaml::<RawChain>(&file, &mut problems) {
            for s in &raw.steps {
                if !ids.contains(s.as_str()) {
                    problems.push(Problem {
                        file: file.clone(),
                        field: "steps".into(),
                        message: format!("unknown command id `{s}`"),
                    });
                }
            }
            chains.push(Chain {
                id: raw.id,
                title: raw.title,
                steps: raw.steps,
            });
        }
    }

    let mut errors = Vec::new();
    let errors_file = dost.join("errors.yml");
    if errors_file.is_file() {
        if let Some(raws) = read_yaml::<Vec<RawError>>(&errors_file, &mut problems) {
            for r in raws {
                match Regex::new(&r.pattern) {
                    Ok(re) => errors.push(ErrorRule {
                        match_: re,
                        module: r.module,
                        cause: r.cause,
                        fix: r.fix,
                        link: r.link,
                    }),
                    Err(e) => problems.push(Problem {
                        file: errors_file.clone(),
                        field: "match".into(),
                        message: format!("invalid regex `{}`: {e}", r.pattern),
                    }),
                }
            }
        }
    }

    let mut scenes = Vec::new();
    for file in yaml_files(&dost.join("scenes")) {
        if let Some(def) = read_yaml::<SceneDef>(&file, &mut problems) {
            scenes.push(def);
        }
    }

    let theme_file = dost.join("theme.yml");
    let theme = if theme_file.is_file() {
        read_yaml::<Theme>(&theme_file, &mut problems).unwrap_or_default()
    } else {
        Theme::default()
    };

    if !problems.is_empty() {
        return Err(CatalogError::Many(problems));
    }
    Ok(Project {
        root: root.to_path_buf(),
        commands: commands.into_iter().map(|(_, c)| c).collect(),
        chains,
        errors,
        scenes,
        theme,
    })
}
