//! Tiny path language used by scene bindings: dotted keys and `[*]` wildcards.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    /// `name[*]` (or a bare `[*]`): every value of an object or element of an array.
    Wild(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("bad path `{path}`: {reason}")]
pub struct BindError {
    pub path: String,
    pub reason: String,
}

pub fn parse_path(path: &str) -> Result<Vec<Seg>, BindError> {
    let err = |reason: &str| BindError {
        path: path.to_string(),
        reason: reason.to_string(),
    };
    if path.is_empty() {
        return Err(err("empty path"));
    }
    path.split('.')
        .map(|part| {
            if part.is_empty() {
                return Err(err("empty segment"));
            }
            let (name, wild) = match part.strip_suffix("[*]") {
                Some(n) => (n, true),
                None => (part, false),
            };
            if name.contains('[') || name.contains(']') {
                return Err(err("only `[*]` is supported in brackets"));
            }
            Ok(if wild {
                Seg::Wild(name.to_string())
            } else {
                Seg::Key(name.to_string())
            })
        })
        .collect()
}

/// Values found at `path`. Missing keys give an empty result, not an error.
pub fn resolve_path(root: &Value, path: &str) -> Result<Vec<Value>, BindError> {
    let segs = parse_path(path)?;
    let mut cur = vec![root.clone()];
    for seg in segs {
        let mut next = Vec::new();
        for v in cur {
            match &seg {
                Seg::Key(k) => {
                    if let Some(x) = v.get(k) {
                        next.push(x.clone());
                    }
                }
                Seg::Wild(k) => {
                    let target = if k.is_empty() { Some(&v) } else { v.get(k) };
                    match target {
                        Some(Value::Object(m)) => next.extend(m.values().cloned()),
                        Some(Value::Array(a)) => next.extend(a.iter().cloned()),
                        _ => {}
                    }
                }
            }
        }
        cur = next;
    }
    Ok(cur)
}
