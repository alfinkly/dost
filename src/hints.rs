//! Matching a failure against the project's known-error rules.

use crate::catalog::ErrorRule;

#[derive(Debug, Clone)]
pub struct Failure {
    pub module: Option<String>,
    pub text: String,
}

/// Rules whose regex matches the text and whose `module` (if set) equals the failing module.
pub fn match_errors<'a>(rules: &'a [ErrorRule], failure: &Failure) -> Vec<&'a ErrorRule> {
    rules
        .iter()
        .filter(|r| {
            let module_ok = match (&r.module, &failure.module) {
                (None, _) => true,
                (Some(want), Some(got)) => want == got,
                (Some(_), None) => false,
            };
            module_ok && r.match_.is_match(&failure.text)
        })
        .collect()
}

/// Hints for a whole run: every failed task is matched with its own module and message, and
/// the raw output tail is matched without a module. Each rule appears once.
pub fn hints_for<'a>(
    rules: &'a [ErrorRule],
    state: &crate::stream::RunState,
    tail: &str,
) -> Vec<&'a ErrorRule> {
    let mut failures: Vec<Failure> = state
        .results
        .iter()
        .filter(|r| r.failed || r.unreachable)
        .filter_map(|r| {
            r.msg.clone().map(|text| Failure {
                module: r.module.clone(),
                text,
            })
        })
        .collect();
    failures.push(Failure {
        module: None,
        text: tail.to_string(),
    });
    let mut out: Vec<&ErrorRule> = Vec::new();
    for f in &failures {
        for rule in match_errors(rules, f) {
            if !out.iter().any(|x| std::ptr::eq(*x, rule)) {
                out.push(rule);
            }
        }
    }
    out
}
