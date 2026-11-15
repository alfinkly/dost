//! "Ask Claude" for a failed run: a headless, read-only `claude -p` call that returns either a
//! patch to files of this project or steps to take. Nothing is executed or edited by Claude;
//! dost applies a patch itself, only after the operator confirms.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::stream::RunMode;

#[derive(Debug, Clone)]
pub struct AssistantConfig {
    pub command: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub max_budget_usd: f32,
    pub timeout_secs: u64,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        AssistantConfig {
            command: "claude".into(),
            model: None,
            tools: vec!["Read".into(), "Grep".into(), "Glob".into()],
            max_budget_usd: 0.5,
            timeout_secs: 180,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssistantError {
    #[error("`{0}` was not found or could not be started; install Claude Code or set assistant.command in .dost/config.yml")]
    NotStarted(String),
    #[error("Claude is not signed in: run `claude` once in a terminal and sign in, then ask again ({0})")]
    NotSignedIn(String),
    #[error("Claude answered with an error: {0}")]
    Reported(String),
    #[error("claude exited with {code}: {stderr}")]
    Failed { code: i32, stderr: String },
    #[error("the request was cancelled")]
    Cancelled,
    #[error("Claude took longer than {0} seconds")]
    Timeout(u64),
    #[error("the answer was not usable: {0}")]
    BadAnswer(String),
}

/// What the operator's failed run looked like.
#[derive(Debug, Clone)]
pub struct HelpContext {
    pub command_id: String,
    pub title: String,
    pub about: String,
    pub playbook: PathBuf,
    pub mode: RunMode,
    pub failures: Vec<String>,
    pub output_tail: String,
    pub catalog_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub text: String,
    /// A catalog command id dost can open for you.
    #[serde(default)]
    pub run: Option<String>,
    /// Text the operator may copy (an ansible-playbook line, a read-only check).
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Proposal {
    Patch {
        title: String,
        explanation: String,
        diff: String,
    },
    Steps {
        title: String,
        explanation: String,
        steps: Vec<Step>,
    },
}

impl Proposal {
    pub fn title(&self) -> &str {
        match self {
            Proposal::Patch { title, .. } | Proposal::Steps { title, .. } => title,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Advice {
    pub summary: String,
    pub cause: String,
    pub proposals: Vec<Proposal>,
}

#[derive(Deserialize)]
struct RawProposal {
    kind: String,
    title: String,
    #[serde(default)]
    explanation: String,
    #[serde(default)]
    diff: Option<String>,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct RawAdvice {
    summary: String,
    cause: String,
    proposals: Vec<RawProposal>,
}

pub const SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"cause":{"type":"string"},"proposals":{"type":"array","items":{"type":"object","properties":{"kind":{"type":"string","enum":["patch","steps"]},"title":{"type":"string"},"explanation":{"type":"string"},"diff":{"type":"string"},"steps":{"type":"array","items":{"type":"object","properties":{"text":{"type":"string"},"run":{"type":"string"},"command":{"type":"string"}},"required":["text"]}}},"required":["kind","title"]}}},"required":["summary","cause","proposals"]}"#;

/// Hide values that look like secrets before anything leaves this machine.
pub fn scrub(text: &str) -> String {
    let key_value = Regex::new(
        r#"(?i)\b([\w.-]*(?:pass(?:word|wd)?|token|secret|api[_-]?key|private[_-]?key|credential)[\w.-]*)(\s*[:=]\s*)("[^"]*"|'[^']*'|\S+)"#,
    )
    .unwrap();
    let bearer = Regex::new(r"(?i)(bearer|basic)\s+[A-Za-z0-9._~+/=-]+").unwrap();
    let pem =
        Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
            .unwrap();
    let t = pem.replace_all(text, "<redacted private key>");
    let t = bearer.replace_all(&t, "$1 <redacted>");
    key_value.replace_all(&t, "$1$2<redacted>").into_owned()
}

pub fn build_prompt(ctx: &HelpContext) -> String {
    let mode = match ctx.mode {
        RunMode::Check => "check (--check --diff, nothing changed)",
        RunMode::Apply => "apply",
    };
    let failures = if ctx.failures.is_empty() {
        "(none listed)".to_string()
    } else {
        ctx.failures.join("\n")
    };
    let prompt = format!(
        "You are helping an operator whose Ansible run, driven by the terminal player `dost`, failed.\n\
         \n\
         Rules:\n\
         - Servers are changed only through Ansible playbooks and roles of this repository. Never suggest \
         editing or restarting things on a server by hand; a fix is a change to repository files or an \
         Ansible run.\n\
         - You can read files here (Read, Grep, Glob) but cannot run commands or edit anything. Look at the \
         playbook and the roles it uses before you answer.\n\
         - Answer with the JSON the schema asks for. Give a short `summary` of what happened and the most \
         likely `cause`.\n\
         - `proposals`: at most 3, best first. Use kind `patch` when the cause is in the repository: `diff` \
         is a unified diff with paths relative to the project root and a/ b/ prefixes, minimal, touching only \
         files that exist here. Use kind `steps` otherwise: each step has `text`; set `run` to one of the \
         catalog command ids below when dost can run that command for the operator; set `command` only for \
         text the operator may copy (an ansible-playbook line or a read-only check).\n\
         - Never ask for passwords or secrets.\n\
         \n\
         Failed command: {id} - {title}\n\
         About: {about}\n\
         Playbook: {playbook}\n\
         Mode: {mode}\n\
         Catalog command ids: {ids}\n\
         \n\
         What failed:\n{failures}\n\
         \n\
         Last output lines:\n{tail}\n",
        id = ctx.command_id,
        title = ctx.title,
        about = ctx.about,
        playbook = ctx.playbook.display(),
        ids = ctx.catalog_ids.join(", "),
        tail = ctx.output_tail,
    );
    scrub(&prompt)
}

fn extract_json(text: &str) -> Option<String> {
    let t = text.trim();
    if t.starts_with('{') {
        return Some(t.to_string());
    }
    if let Some(start) = t.find("```") {
        let after = &t[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        if let Some(end) = after.find("```") {
            let body = after[..end].trim();
            if body.starts_with('{') {
                return Some(body.to_string());
            }
        }
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    (b > a).then(|| t[a..=b].to_string())
}

fn into_advice(raw: RawAdvice) -> Result<Advice, AssistantError> {
    let mut proposals = Vec::new();
    for p in raw.proposals {
        match p.kind.as_str() {
            "patch" => {
                let diff = p.diff.filter(|d| !d.trim().is_empty()).ok_or_else(|| {
                    AssistantError::BadAnswer(format!("patch `{}` has no diff", p.title))
                })?;
                proposals.push(Proposal::Patch {
                    title: p.title,
                    explanation: p.explanation,
                    diff,
                });
            }
            "steps" => proposals.push(Proposal::Steps {
                title: p.title,
                explanation: p.explanation,
                steps: p.steps,
            }),
            other => {
                return Err(AssistantError::BadAnswer(format!(
                    "unknown proposal kind `{other}`"
                )))
            }
        }
    }
    Ok(Advice {
        summary: raw.summary,
        cause: raw.cause,
        proposals,
    })
}

/// Read the JSON envelope printed by `claude -p --output-format json`.
pub fn parse_envelope(stdout: &str) -> Result<Advice, AssistantError> {
    let env: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| AssistantError::BadAnswer(format!("not JSON: {e}")))?;
    let result_text = env.get("result").and_then(|v| v.as_str()).unwrap_or("");
    if env.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
        let low = result_text.to_lowercase();
        return Err(
            if low.contains("authenticate") || low.contains("oauth") || low.contains("login") {
                AssistantError::NotSignedIn(result_text.to_string())
            } else {
                AssistantError::Reported(result_text.to_string())
            },
        );
    }
    let body = match env.get("structured_output") {
        Some(v) if v.is_object() => v.to_string(),
        _ => extract_json(result_text)
            .ok_or_else(|| AssistantError::BadAnswer("no JSON in the answer".into()))?,
    };
    let raw: RawAdvice =
        serde_json::from_str(&body).map_err(|e| AssistantError::BadAnswer(e.to_string()))?;
    into_advice(raw)
}

/// Files a patch touches, after refusing anything outside the project.
pub fn validate_patch(diff: &str, _root: &Path) -> Result<Vec<PathBuf>, String> {
    if diff.trim().is_empty() {
        return Err("the patch is empty".into());
    }
    let mut files = Vec::new();
    let mut saw_header = false;
    for line in diff.lines() {
        let Some(rest) = line.strip_prefix("+++ ") else {
            if let Some(old) = line.strip_prefix("--- ") {
                saw_header = true;
                check_path(old.split('\t').next().unwrap_or(old), true)?;
            }
            continue;
        };
        saw_header = true;
        let path = rest.split('\t').next().unwrap_or(rest);
        if path == "/dev/null" {
            return Err("deleting files is not allowed".into());
        }
        files.push(check_path(path, false)?);
    }
    if !saw_header || files.is_empty() {
        return Err("not a unified diff".into());
    }
    Ok(files)
}

fn check_path(raw: &str, old_side: bool) -> Result<PathBuf, String> {
    if raw == "/dev/null" && old_side {
        return Ok(PathBuf::new()); // a new file
    }
    let stripped = raw
        .strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .ok_or_else(|| format!("path `{raw}` must start with a/ or b/"))?;
    let p = PathBuf::from(stripped);
    if stripped.is_empty() || p.is_absolute() || stripped.starts_with('/') {
        return Err(format!("path `{stripped}` must be relative to the project"));
    }
    for c in p.components() {
        match c {
            std::path::Component::Normal(n) if n == ".git" => {
                return Err("changing .git is not allowed".into())
            }
            std::path::Component::Normal(_) => {}
            _ => return Err(format!("path `{stripped}` leaves the project")),
        }
    }
    Ok(p)
}

fn git_apply(root: &Path, diff: &str, reverse: bool, check_only: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("apply").arg("--whitespace=nowarn");
    if reverse {
        cmd.arg("-R");
    }
    if check_only {
        cmd.arg("--check");
    }
    let mut child = cmd
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git is needed to apply patches: {e}"))?;
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(diff.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Apply a patch to the project. It is validated and test-applied first; a patch that does not
/// apply cleanly changes nothing.
pub fn apply_patch(root: &Path, diff: &str) -> Result<(), String> {
    validate_patch(diff, root)?;
    let diff = if diff.ends_with('\n') {
        diff.to_string()
    } else {
        format!("{diff}\n")
    };
    git_apply(root, &diff, false, true)?;
    git_apply(root, &diff, false, false)
}

pub fn revert_patch(root: &Path, diff: &str) -> Result<(), String> {
    let diff = if diff.ends_with('\n') {
        diff.to_string()
    } else {
        format!("{diff}\n")
    };
    git_apply(root, &diff, true, true)?;
    git_apply(root, &diff, true, false)
}

/// Run `claude -p` read-only in the project directory and parse its answer.
pub async fn ask(
    cfg: &AssistantConfig,
    prompt: &str,
    root: &Path,
    cancel: CancellationToken,
) -> Result<Advice, AssistantError> {
    let mut cmd = tokio::process::Command::new(&cfg.command);
    cmd.arg("-p")
        .args(["--output-format", "json"])
        .args(["--tools", &cfg.tools.join(",")])
        .args(["--permission-prompts", "none"])
        .arg("--no-session-persistence")
        .args(["--max-budget-usd", &cfg.max_budget_usd.to_string()])
        .args(["--json-schema", SCHEMA]);
    if let Some(m) = &cfg.model {
        cmd.args(["--model", m]);
    }
    cmd.current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|_| AssistantError::NotStarted(cfg.command.clone()))?;
    let mut stdin = child.stdin.take().unwrap();
    let prompt = prompt.to_string();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });
    let wait = child.wait_with_output();
    tokio::pin!(wait);
    let out = tokio::select! {
        r = &mut wait => r.map_err(|e| AssistantError::Failed { code: -1, stderr: e.to_string() })?,
        _ = cancel.cancelled() => return Err(AssistantError::Cancelled),
        _ = tokio::time::sleep(Duration::from_secs(cfg.timeout_secs)) => {
            return Err(AssistantError::Timeout(cfg.timeout_secs))
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        // an error envelope still carries the reason on stdout
        if let Err(e @ (AssistantError::NotSignedIn(_) | AssistantError::Reported(_))) =
            parse_envelope(&stdout)
        {
            return Err(e);
        }
        return Err(AssistantError::Failed {
            code: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    parse_envelope(&stdout)
}

#[derive(Deserialize, Default)]
struct RawConfigFile {
    #[serde(default)]
    assistant: Option<RawAssistant>,
}

#[derive(Deserialize, Default)]
struct RawAssistant {
    command: Option<String>,
    model: Option<String>,
    tools: Option<Vec<String>>,
    max_budget_usd: Option<f32>,
    timeout_secs: Option<u64>,
}

/// `assistant:` from `.dost/config.yml`; a missing or broken file gives the defaults.
pub fn load_config(root: &Path) -> AssistantConfig {
    let mut cfg = AssistantConfig::default();
    let Ok(text) = std::fs::read_to_string(root.join(".dost/config.yml")) else {
        return cfg;
    };
    let Ok(file) = serde_yaml_ng::from_str::<RawConfigFile>(&text) else {
        return cfg;
    };
    if let Some(a) = file.assistant {
        if let Some(c) = a.command {
            cfg.command = c;
        }
        cfg.model = a.model.or(cfg.model);
        if let Some(t) = a.tools {
            cfg.tools = t;
        }
        if let Some(b) = a.max_budget_usd {
            cfg.max_budget_usd = b;
        }
        if let Some(t) = a.timeout_secs {
            cfg.timeout_secs = t;
        }
    }
    cfg
}
