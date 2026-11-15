//! Spawning `ansible-playbook` with the dost callback and streaming its output.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use nix::fcntl::{fcntl, FcntlArg, FdFlag};
use nix::sys::signal::{killpg, Signal};
use nix::unistd::{pipe, Pid};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::catalog::Command;
use crate::secret::BecomePassword;
use crate::stream::{parse_line, RunMode, StreamLine};

const TAIL_LINES: usize = 100;
/// Longest line kept in memory; the rest of a longer line is read and dropped.
const MAX_LINE_BYTES: usize = 256 * 1024;

/// Next line as lossy UTF-8, truncated to `MAX_LINE_BYTES`. Never fails on bad bytes and
/// always drains the whole line, so the child cannot block on a full pipe.
async fn read_line_capped<R: AsyncBufRead + Unpin>(r: &mut R) -> Option<String> {
    let mut kept: Vec<u8> = Vec::new();
    let mut got_any = false;
    loop {
        let chunk = match r.fill_buf().await {
            Ok(c) => c,
            Err(_) => return got_any.then(|| String::from_utf8_lossy(&kept).into_owned()),
        };
        if chunk.is_empty() {
            return got_any.then(|| String::from_utf8_lossy(&kept).into_owned());
        }
        got_any = true;
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        let room = MAX_LINE_BYTES.saturating_sub(kept.len());
        let body = &chunk[..take - usize::from(done)];
        kept.extend_from_slice(&body[..body.len().min(room)]);
        r.consume(take);
        if done {
            return Some(String::from_utf8_lossy(&kept).into_owned());
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunPlan {
    pub argv: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub mode: RunMode,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    /// Last lines of stderr and of stdout lines that were not stream lines.
    pub output_tail: String,
}

pub fn build_plan(root: &Path, cmd: &Command, mode: RunMode, plugin_dir: &Path) -> RunPlan {
    let mut argv: Vec<OsString> = vec!["ansible-playbook".into(), cmd.playbook.clone().into()];
    if mode == RunMode::Check {
        argv.push("--check".into());
        argv.push("--diff".into());
    }
    if let Some(inv) = &cmd.inventory {
        argv.extend(["-i".into(), inv.into()]);
    }
    if let Some(limit) = &cmd.limit {
        argv.extend(["--limit".into(), limit.into()]);
    }
    if !cmd.tags.is_empty() {
        argv.extend(["--tags".into(), cmd.tags.join(",").into()]);
    }
    for v in &cmd.extra_vars {
        argv.extend(["-e".into(), v.into()]);
    }
    let mode_name = match mode {
        RunMode::Check => "check",
        RunMode::Apply => "apply",
    };
    let env = vec![
        (
            "ANSIBLE_CALLBACK_PLUGINS".to_string(),
            plugin_dir.to_string_lossy().to_string(),
        ),
        (
            "ANSIBLE_STDOUT_CALLBACK".to_string(),
            "dost_stream".to_string(),
        ),
        ("DOST_MODE".to_string(), mode_name.to_string()),
        ("ANSIBLE_NOCOLOR".to_string(), "1".to_string()),
    ];
    RunPlan {
        argv,
        env,
        mode,
        cwd: root.to_path_buf(),
    }
}

fn quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:=,@%+-".contains(c));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command line as shown in the confirmation screen. Contains no secrets.
pub fn display_command(plan: &RunPlan) -> String {
    plan.argv
        .iter()
        .map(|a| quote(&a.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_tail(tail: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut t = tail.lock().unwrap();
    if t.len() == TAIL_LINES {
        t.pop_front();
    }
    t.push_back(line);
}

/// Hand the password to the child through an anonymous pipe. Returns the read end,
/// which the child inherits as `/dev/fd/N`; nothing is written to disk.
fn password_pipe(pw: &BecomePassword) -> Result<std::os::fd::OwnedFd> {
    let (r, w) = pipe().context("create pipe")?;
    let mut f = File::from(w);
    f.write_all(pw.expose()).context("write password to pipe")?;
    drop(f);
    Ok(r)
}

pub async fn run(
    plan: RunPlan,
    become_pw: Option<BecomePassword>,
    tx: mpsc::Sender<StreamLine>,
    cancel: CancellationToken,
) -> Result<RunOutcome> {
    let mut argv = plan.argv.clone();
    let pw_read = match &become_pw {
        Some(pw) => {
            if !pw.is_ansible_safe() {
                anyhow::bail!(
                    "Ansible strips spaces around the password; passwords with leading or trailing whitespace are not supported"
                );
            }
            let r = password_pipe(pw)?;
            argv.push("--become-password-file".into());
            argv.push(format!("/dev/fd/{}", r.as_raw_fd()).into());
            Some(r)
        }
        None => None,
    };

    let mut command = tokio::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .envs(plan.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(&plan.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("start {}", argv[0].to_string_lossy()))?;

    if let Some(r) = &pw_read {
        // The child has its copy; keep ours from leaking into later children.
        let _ = fcntl(r.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC));
    }
    drop(pw_read);
    drop(become_pw);

    let pid = child.id().map(|p| Pid::from_raw(p as i32));
    let tail = Arc::new(Mutex::new(VecDeque::new()));

    let stdout = child.stdout.take().unwrap();
    let t1 = tail.clone();
    let out_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        while let Some(line) = read_line_capped(&mut reader).await {
            match parse_line(&line) {
                Some(l) => {
                    let _ = tx.send(l).await;
                }
                None => push_tail(&t1, line),
            }
        }
    });
    let stderr = child.stderr.take().unwrap();
    let t2 = tail.clone();
    let err_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        while let Some(line) = read_line_capped(&mut reader).await {
            push_tail(&t2, line);
        }
    });

    let mut cancelled = false;
    let status = tokio::select! {
        s = child.wait() => s?,
        _ = cancel.cancelled() => {
            cancelled = true;
            if let Some(p) = pid { let _ = killpg(p, Signal::SIGTERM); }
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(s) => s?,
                Err(_) => {
                    if let Some(p) = pid { let _ = killpg(p, Signal::SIGKILL); }
                    child.wait().await?
                }
            }
        }
    };
    let _ = out_task.await;
    let _ = err_task.await;

    let output_tail = tail
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(RunOutcome {
        exit_code: status.code(),
        cancelled,
        output_tail,
    })
}
