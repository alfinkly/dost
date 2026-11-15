use dost::catalog::{find_root, load_project, CatalogError, Danger};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, body) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
    }
    dir
}

const PERIMETER: &str = "id: perimeter\ntitle: Perimeter\nplaybook: playbooks/perimeter.yml\nabout: Closes ports\ndanger: medium\nneeds_become: true\nscene: ports\nafter: [base]\ndone: \"Done on {{ hosts | join(', ') }}\"\n";
const BASE: &str = "id: base\ntitle: Base\nplaybook: playbooks/base.yml\nabout: Base setup\n";

fn good() -> TempDir {
    project(&[
        ("playbooks/perimeter.yml", "- hosts: all\n"),
        ("playbooks/base.yml", "- hosts: all\n"),
        (".dost/catalog/perimeter.yml", PERIMETER),
        (".dost/catalog/base.yml", BASE),
        (
            ".dost/catalog/chains/node.yml",
            "id: node\ntitle: Node\nsteps: [base, perimeter]\n",
        ),
        (
            ".dost/errors.yml",
            "- match: \"Too many authentication failures\"\n  cause: SSH limit\n  fix: Use ControlMaster\n",
        ),
    ])
}

#[test]
fn loads_example_project() {
    let dir = good();
    let p = load_project(dir.path()).unwrap();
    assert_eq!(p.commands.len(), 2);
    assert_eq!(p.chains.len(), 1);
    assert_eq!(p.errors.len(), 1);
    let base = p.commands.iter().find(|c| c.id == "base").unwrap();
    assert!(base.check, "check defaults to true");
    assert_eq!(base.danger, Danger::Low);
    let per = p.commands.iter().find(|c| c.id == "perimeter").unwrap();
    assert_eq!(per.danger, Danger::Medium);
    assert!(per.needs_become);
    assert_eq!(per.after, vec!["base".to_string()]);
}

#[test]
fn unknown_after_id_is_error() {
    let dir = project(&[
        ("playbooks/perimeter.yml", "- hosts: all\n"),
        (".dost/catalog/perimeter.yml", PERIMETER),
    ]);
    let err = load_project(dir.path()).unwrap_err();
    let CatalogError::Many(problems) = err else {
        panic!("expected Many")
    };
    assert!(problems
        .iter()
        .any(|p| p.field == "after" && p.file.ends_with("perimeter.yml")));
}

#[test]
fn missing_playbook_file_is_error() {
    let dir = project(&[(".dost/catalog/base.yml", BASE)]);
    let CatalogError::Many(problems) = load_project(dir.path()).unwrap_err() else {
        panic!("expected Many")
    };
    assert!(problems.iter().any(|p| p.field == "playbook"));
}

#[test]
fn chain_with_unknown_step_is_error() {
    let dir = project(&[
        ("playbooks/base.yml", "- hosts: all\n"),
        (".dost/catalog/base.yml", BASE),
        (
            ".dost/catalog/chains/c.yml",
            "id: c\ntitle: C\nsteps: [base, nope]\n",
        ),
    ]);
    let CatalogError::Many(problems) = load_project(dir.path()).unwrap_err() else {
        panic!("expected Many")
    };
    assert!(problems.iter().any(|p| p.field == "steps"));
}

#[test]
fn invalid_error_regex_is_error() {
    let dir = project(&[
        ("playbooks/base.yml", "- hosts: all\n"),
        (".dost/catalog/base.yml", BASE),
        (
            ".dost/errors.yml",
            "- match: \"(unclosed\"\n  cause: x\n  fix: y\n",
        ),
    ]);
    let CatalogError::Many(problems) = load_project(dir.path()).unwrap_err() else {
        panic!("expected Many")
    };
    assert!(problems.iter().any(|p| p.field == "match"));
}

#[test]
fn reports_all_problems_not_just_the_first() {
    let dir = project(&[
        (".dost/catalog/base.yml", BASE),
        (
            ".dost/errors.yml",
            "- match: \"(unclosed\"\n  cause: x\n  fix: y\n",
        ),
    ]);
    let CatalogError::Many(problems) = load_project(dir.path()).unwrap_err() else {
        panic!("expected Many")
    };
    assert!(problems.len() >= 2);
}

#[test]
fn find_root_walks_up_to_dot_dost() {
    let dir = good();
    let nested = dir.path().join("playbooks");
    assert_eq!(
        find_root(&nested).unwrap().canonicalize().unwrap(),
        dir.path().canonicalize().unwrap()
    );
    assert!(find_root(Path::new("/")).is_none());
}
