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
