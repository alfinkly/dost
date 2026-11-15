use dost::assistant::{
    apply_patch, ask, build_prompt, parse_envelope, revert_patch, scrub, validate_patch, Advice,
    AssistantConfig, HelpContext, Proposal,
};
use dost::stream::RunMode;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn ctx() -> HelpContext {
    HelpContext {
        command_id: "gpu-chain".into(),
        title: "GPU chain".into(),
        about: "walks the chain".into(),
        playbook: PathBuf::from("playbooks/services/gpu-chain.yml"),
        mode: RunMode::Check,
        failures: vec![
            "✗ host: gpu-tunnel.service: activating".into(),
            "token=SUPERSECRET123 leaked".into(),
        ],
        output_tail: "bind: Address already in use\nAuthorization: Bearer abc.def.ghi".into(),
        catalog_ids: vec!["openbao".into(), "gpu-tunnel".into()],
    }
}

#[test]
fn prompt_has_the_failure_the_ids_the_rules_and_no_secrets() {
    let p = build_prompt(&ctx());
    for needle in [
        "gpu-chain",
        "playbooks/services/gpu-chain.yml",
        "activating",
        "Address already in use",
        "openbao",
        "gpu-tunnel",
        "only through Ansible",
    ] {
        assert!(p.contains(needle), "prompt misses {needle}:\n{p}");
    }
    assert!(
        !p.contains("SUPERSECRET123") && !p.contains("abc.def.ghi"),
        "secrets must be scrubbed:\n{p}"
    );
}

#[test]
fn scrub_hides_values_of_secret_looking_keys_tokens_and_private_keys() {
    let s = scrub("password: hunter2\nDB_PASSWORD=abc\napi_key = \"k-123\"\nAuthorization: Bearer xyz\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\nplain line");
    for leaked in ["hunter2", "abc", "k-123", "xyz", "AAAA"] {
        assert!(!s.contains(leaked), "{leaked} leaked: {s}");
    }
    assert!(s.contains("plain line") && s.contains("<redacted>"));
}

#[test]
fn parse_envelope_reads_structured_output_fenced_json_and_plain_json() {
    let advice = r#"{"summary":"s","cause":"c","proposals":[{"kind":"steps","title":"t","steps":[{"text":"do it","run":"openbao"},{"text":"copy me","command":"ansible-playbook x.yml --check"}]}]}"#;
    let structured =
        format!(r#"{{"type":"result","is_error":false,"result":"","structured_output":{advice}}}"#);
    let a = parse_envelope(&structured).unwrap();
    assert_eq!(a.summary, "s");
    assert!(
        matches!(&a.proposals[0], Proposal::Steps { steps, .. } if steps.len() == 2 && steps[0].run.as_deref() == Some("openbao") && steps[1].command.is_some())
    );

    let fenced = serde_json::json!({"type":"result","is_error":false,"result": format!("Here you go:\n```json\n{advice}\n```")}).to_string();
    assert_eq!(parse_envelope(&fenced).unwrap().cause, "c");
    let plain = serde_json::json!({"type":"result","is_error":false,"result": advice}).to_string();
    assert_eq!(parse_envelope(&plain).unwrap().summary, "s");
}

#[test]
fn parse_envelope_turns_errors_into_clear_messages() {
    let auth = r#"{"type":"result","is_error":true,"result":"Failed to authenticate: OAuth session expired"}"#;
    let e = parse_envelope(auth).unwrap_err().to_string();
    assert!(e.contains("not signed in") || e.contains("sign in"), "{e}");
    let e = parse_envelope(r#"{"type":"result","is_error":true,"result":"rate limited"}"#)
        .unwrap_err()
        .to_string();
    assert!(e.contains("rate limited"), "{e}");
    assert!(parse_envelope("not json at all").is_err());
    assert!(
        parse_envelope(r#"{"type":"result","is_error":false,"result":"no json here"}"#).is_err()
    );
    let bad_kind = r#"{"type":"result","is_error":false,"result":"{\"summary\":\"s\",\"cause\":\"c\",\"proposals\":[{\"kind\":\"rm-rf\",\"title\":\"t\"}]}"}"#;
    assert!(
        parse_envelope(bad_kind).is_err(),
        "unknown proposal kinds are rejected"
    );
}

const DIFF: &str = "--- a/playbooks/p.yml\n+++ b/playbooks/p.yml\n@@ -1,2 +1,2 @@\n - hosts: all\n-  tasks: []\n+  tasks: [ok]\n";

#[test]
fn validate_patch_accepts_project_files_and_rejects_escapes_deletes_and_git() {
    let root = TempDir::new().unwrap();
    assert_eq!(
        validate_patch(DIFF, root.path()).unwrap(),
        vec![PathBuf::from("playbooks/p.yml")]
    );
    for bad in [
        "--- a/../etc/passwd\n+++ b/../etc/passwd\n@@ -1 +1 @@\n-a\n+b\n",
        "--- a//etc/hosts\n+++ b//etc/hosts\n@@ -1 +1 @@\n-a\n+b\n",
        "--- a/.git/config\n+++ b/.git/config\n@@ -1 +1 @@\n-a\n+b\n",
        "--- a/x.yml\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\n",
        "not a diff",
        "",
    ] {
        assert!(
            validate_patch(bad, root.path()).is_err(),
            "must reject: {bad:?}"
        );
    }
}

#[test]
fn apply_patch_changes_the_file_and_revert_restores_it_and_bad_patches_change_nothing() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("playbooks")).unwrap();
    let file = root.path().join("playbooks/p.yml");
    std::fs::write(&file, "- hosts: all\n  tasks: []\n").unwrap();
    apply_patch(root.path(), DIFF).unwrap();
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("tasks: [ok]"));
    revert_patch(root.path(), DIFF).unwrap();
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("tasks: []"));
    let stale = "--- a/playbooks/p.yml\n+++ b/playbooks/p.yml\n@@ -1,2 +1,2 @@\n - hosts: nothing\n-  tasks: [zzz]\n+  tasks: [ok]\n";
    assert!(apply_patch(root.path(), stale).is_err());
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("tasks: []"),
        "a patch that does not apply changes nothing"
    );
}

fn fake_claude(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-claude");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[tokio::test]
async fn ask_runs_claude_read_only_with_the_prompt_on_stdin_in_the_project_dir() {
    let dir = TempDir::new().unwrap();
    let proj = TempDir::new().unwrap();
    let log = dir.path().join("log");
    let script = fake_claude(
        dir.path(),
        &format!(
            r#"echo "$@" > {log}
pwd >> {log}
cat >> {log}
echo '{{"type":"result","is_error":false,"result":"","structured_output":{{"summary":"s","cause":"c","proposals":[]}}}}'"#,
            log = log.display()
        ),
    );
    let cfg = AssistantConfig {
        command: script.to_string_lossy().into(),
        ..AssistantConfig::default()
    };
    let advice: Advice = ask(
        &cfg,
        "THE-PROMPT-TEXT",
        proj.path(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(advice.summary, "s");
    let seen = std::fs::read_to_string(&log).unwrap();
    for needle in [
        "-p",
        "--output-format json",
        "--tools Read,Grep,Glob",
        "--json-schema",
        "--no-session-persistence",
        "--permission-prompts none",
        "--max-budget-usd",
        "THE-PROMPT-TEXT",
    ] {
        assert!(seen.contains(needle), "missing {needle}:\n{seen}");
    }
    assert!(
        seen.contains(proj.path().file_name().unwrap().to_str().unwrap()),
        "runs in the project dir:\n{seen}"
    );
    assert!(
        !seen.contains("Bash") && !seen.contains("Edit") && !seen.contains("Write"),
        "no write or shell tools:\n{seen}"
    );
}

#[tokio::test]
async fn ask_reports_a_missing_binary_a_failure_exit_and_cancellation_clearly() {
    let proj = TempDir::new().unwrap();
    let cfg = AssistantConfig {
        command: "/nonexistent/claude".into(),
        ..AssistantConfig::default()
    };
    let e = ask(&cfg, "x", proj.path(), CancellationToken::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("claude") && (e.contains("not found") || e.contains("start")),
        "{e}"
    );

    let dir = TempDir::new().unwrap();
    let failing = fake_claude(dir.path(), "echo boom >&2\nexit 3");
    let cfg = AssistantConfig {
        command: failing.to_string_lossy().into(),
        ..AssistantConfig::default()
    };
    assert!(ask(&cfg, "x", proj.path(), CancellationToken::new())
        .await
        .unwrap_err()
        .to_string()
        .contains("boom"));

    let slow = fake_claude(dir.path(), "sleep 30");
    let cfg = AssistantConfig {
        command: slow.to_string_lossy().into(),
        ..AssistantConfig::default()
    };
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        c2.cancel();
    });
    let started = std::time::Instant::now();
    assert!(ask(&cfg, "x", proj.path(), cancel)
        .await
        .unwrap_err()
        .to_string()
        .contains("cancel"));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn config_defaults_are_read_only_and_cheap() {
    let c = AssistantConfig::default();
    assert_eq!(c.command, "claude");
    assert_eq!(
        c.tools,
        vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]
    );
    assert!(c.max_budget_usd <= 1.0 && c.timeout_secs >= 60);
}

#[test]
fn config_is_read_from_dot_dost_config_yml_and_falls_back_to_defaults() {
    use dost::assistant::load_config;
    let d = TempDir::new().unwrap();
    assert_eq!(load_config(d.path()).command, "claude");
    std::fs::create_dir_all(d.path().join(".dost")).unwrap();
    std::fs::write(
        d.path().join(".dost/config.yml"),
        "assistant:\n  command: /opt/claude\n  model: sonnet\n  tools: [Read]\n  max_budget_usd: 0.2\n  timeout_secs: 90\n",
    )
    .unwrap();
    let c = load_config(d.path());
    assert_eq!(
        (c.command.as_str(), c.model.as_deref(), c.timeout_secs),
        ("/opt/claude", Some("sonnet"), 90)
    );
    assert_eq!(c.tools, vec!["Read".to_string()]);
    std::fs::write(d.path().join(".dost/config.yml"), "assistant: [broken").unwrap();
    assert_eq!(
        load_config(d.path()).command,
        "claude",
        "a broken file falls back to the defaults"
    );
}

#[test]
fn an_error_envelope_without_a_result_text_still_says_what_went_wrong() {
    let budget = r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"result":"","total_cost_usd":0.51}"#;
    let e = parse_envelope(budget).unwrap_err().to_string();
    assert!(e.contains("budget") && e.contains("max_budget_usd"), "{e}");
    let turns = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
    assert!(parse_envelope(turns)
        .unwrap_err()
        .to_string()
        .contains("turns"));
    let api = r#"{"type":"result","subtype":"success","is_error":true,"result":"","api_error_status":529}"#;
    let e = parse_envelope(api).unwrap_err().to_string();
    assert!(e.contains("529"), "{e}");
    let nothing = r#"{"type":"result","is_error":true,"weird_field":"x"}"#;
    let e = parse_envelope(nothing).unwrap_err().to_string();
    assert!(
        e.contains("weird_field"),
        "an unknown error shape shows the raw envelope: {e}"
    );
    assert!(!e.trim_end().ends_with(':'), "never an empty message: {e}");
}

#[test]
fn default_budget_leaves_room_for_reading_a_few_files() {
    assert!(AssistantConfig::default().max_budget_usd >= 1.0);
}
