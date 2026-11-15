use dost::catalog::ErrorRule;
use dost::hints::{match_errors, Failure};
use regex::Regex;

fn rule(pat: &str, module: Option<&str>) -> ErrorRule {
    ErrorRule {
        match_: Regex::new(pat).unwrap(),
        module: module.map(String::from),
        cause: "c".into(),
        fix: "f".into(),
        link: None,
    }
}

#[test]
fn regex_and_module_filter_match() {
    let rules = vec![
        rule(
            "Too many authentication failures",
            Some("ansible.builtin.ping"),
        ),
        rule("disk full|No space left", None),
    ];
    let f = Failure {
        module: Some("ansible.builtin.ping".into()),
        text: "UNREACHABLE: Too many authentication failures".into(),
    };
    let hits = match_errors(&rules, &f);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].match_.as_str().contains("Too many"));

    let other = Failure {
        module: Some("ansible.builtin.copy".into()),
        text: "Too many authentication failures".into(),
    };
    assert!(match_errors(&rules, &other).is_empty());
}

#[test]
fn rule_without_module_matches_any() {
    let rules = vec![rule("No space left", None)];
    let f = Failure {
        module: Some("ansible.builtin.apt".into()),
        text: "write failed: No space left on device".into(),
    };
    assert_eq!(match_errors(&rules, &f).len(), 1);
    let f2 = Failure {
        module: None,
        text: "No space left".into(),
    };
    assert_eq!(match_errors(&rules, &f2).len(), 1);
}
