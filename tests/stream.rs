use dost::stream::{parse_line, RunMode, RunState, StreamLine};

#[test]
fn parses_task_result() {
    let line = r#"{"t":"result","host":"web1","task":"install","changed":true,"failed":false,"skipped":false,"unreachable":false,"msg":"ok","diff":null}"#;
    match parse_line(line) {
        Some(StreamLine::TaskResult {
            host,
            task,
            changed,
            failed,
            ..
        }) => {
            assert_eq!(host, "web1");
            assert_eq!(task, "install");
            assert!(changed);
            assert!(!failed);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn debug_with_dost_key_becomes_event() {
    let line = r#"{"t":"event","host":"web1","v":1,"kind":"ports","key":null,"data":{"state":{"22":"on","80":"off"}}}"#;
    match parse_line(line) {
        Some(StreamLine::Event {
            host, kind, data, ..
        }) => {
            assert_eq!(host, "web1");
            assert_eq!(kind, "ports");
            assert_eq!(data["state"]["80"], "off");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn unknown_kind_is_kept_but_unknown_version_dropped() {
    let kept = r#"{"t":"event","host":"a","v":1,"kind":"weird","key":null,"data":{}}"#;
    assert!(parse_line(kept).is_some());
    let dropped = r#"{"t":"event","host":"a","v":2,"kind":"ports","key":null,"data":{}}"#;
    assert!(parse_line(dropped).is_none());
}

#[test]
fn garbage_and_huge_lines_return_none() {
    assert!(parse_line("not json").is_none());
    assert!(parse_line(r#"{"t":"result","host":5}"#).is_none());
    let huge = format!(
        r#"{{"t":"event","host":"a","v":1,"kind":"note","key":null,"data":{{"x":"{}"}}}}"#,
        "a".repeat(1_100_000)
    );
    assert!(parse_line(&huge).is_none());
}

#[test]
fn same_key_overwrites_different_key_accumulates() {
    let mut st = RunState::default();
    st.apply(StreamLine::RunStart {
        mode: RunMode::Apply,
    });
    let ev = |key: Option<&str>, v: &str| StreamLine::Event {
        host: "h".into(),
        kind: "link".into(),
        key: key.map(String::from),
        data: serde_json::json!({ "state": v }),
    };
    st.apply(ev(Some("a-b"), "down"));
    st.apply(ev(Some("a-b"), "up"));
    st.apply(ev(Some("b-c"), "up"));
    assert_eq!(st.events.len(), 2);
    let k = ("h".to_string(), "link".to_string(), "a-b".to_string());
    assert_eq!(st.events[&k]["state"], "up");
}

#[test]
fn no_log_result_has_no_msg() {
    let line = r#"{"t":"result","host":"h","task":"secret","changed":true,"failed":false,"skipped":false,"unreachable":false,"msg":null,"diff":null,"no_log":true}"#;
    match parse_line(line) {
        Some(StreamLine::TaskResult { msg, diff, .. }) => {
            assert!(msg.is_none());
            assert!(diff.is_none());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn recap_updates_hosts() {
    let mut st = RunState::default();
    st.apply(StreamLine::Recap {
        host: "h".into(),
        ok: 3,
        changed: 1,
        failed: 0,
        unreachable: 0,
    });
    assert!(st.hosts.contains(&"h".to_string()));
    assert_eq!(st.recap["h"].changed, 1);
}

#[test]
fn recap_marks_the_run_finished_and_run_start_keeps_display_settings() {
    let mut st = RunState {
        focus: vec!["x".into()],
        frame: 7,
        ..Default::default()
    };
    st.apply(StreamLine::RunStart {
        mode: RunMode::Check,
    });
    assert!(!st.finished);
    assert_eq!((st.focus.clone(), st.frame), (vec!["x".to_string()], 7));
    st.apply(StreamLine::Recap {
        host: "h".into(),
        ok: 1,
        changed: 0,
        failed: 0,
        unreachable: 0,
    });
    assert!(st.finished);
}

#[test]
fn play_and_item_lines_parse_and_the_state_tracks_the_current_play() {
    assert!(matches!(
        parse_line(r#"{"t":"play","name":"Admin users"}"#),
        Some(StreamLine::Play { name }) if name == "Admin users"
    ));
    let item = r#"{"t":"item","host":"h","task":"Admin accounts","item":"alice","changed":true,"failed":false,"skipped":false,"no_log":false}"#;
    assert!(matches!(
        parse_line(item),
        Some(StreamLine::Item { host, item, changed: true, .. }) if host == "h" && item == "alice"
    ));
    let mut st = RunState::default();
    st.apply(StreamLine::Play {
        name: "Admin users".into(),
    });
    st.apply(parse_line(item).unwrap());
    st.apply(StreamLine::TaskResult {
        host: "h".into(),
        task: "t".into(),
        changed: false,
        failed: false,
        skipped: false,
        unreachable: false,
        module: None,
        msg: None,
        diff: None,
    });
    assert_eq!(st.current_play, "Admin users");
    assert_eq!(st.items.len(), 1);
    assert_eq!(st.items[0].play, "Admin users");
    assert_eq!(st.results[0].play, "Admin users");
}

#[test]
fn item_of_a_no_log_task_carries_no_label() {
    let item = r#"{"t":"item","host":"h","task":"secret","item":null,"changed":true,"failed":false,"skipped":false,"no_log":true}"#;
    assert!(matches!(parse_line(item), Some(StreamLine::Item { item, .. }) if item.is_empty()));
}

use dost::stream::parse_progress_line;

#[test]
fn progress_line_parses_into_an_event_and_ignores_everything_else() {
    let line = r#"{"host":"laptop","dost":{"v":1,"kind":"transfer","key":"files","from":"s3://a","to":"s3://b","state":"running","done":5,"total":10}}"#;
    match parse_progress_line(line) {
        Some(StreamLine::Event {
            host,
            kind,
            key,
            data,
        }) => {
            assert_eq!(
                (host.as_str(), kind.as_str(), key.as_deref()),
                ("laptop", "transfer", Some("files"))
            );
            assert_eq!(data["done"], 5);
            assert!(
                data.get("v").is_none() && data.get("kind").is_none() && data.get("key").is_none()
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
    let no_host = r#"{"dost":{"v":1,"kind":"note","text":"x"}}"#;
    assert!(
        matches!(parse_progress_line(no_host), Some(StreamLine::Event { host, .. }) if host == "local")
    );
    for bad in [
        "",
        "not json",
        r#"{"dost":{"v":2,"kind":"x"}}"#,
        r#"{"other":1}"#,
        r#"{"dost":{"v":1}}"#,
    ] {
        assert!(parse_progress_line(bad).is_none(), "{bad}");
    }
}

#[test]
fn events_remember_arrival_order_and_frame() {
    let mut st = RunState::default();
    let ev = |k: &str, s: &str| StreamLine::Event {
        host: "h".into(),
        kind: "transfer".into(),
        key: Some(k.into()),
        data: serde_json::json!({"state": s}),
    };
    st.frame = 3;
    st.apply(ev("b", "running"));
    st.frame = 9;
    st.apply(ev("a", "running"));
    st.frame = 20;
    st.apply(ev("b", "done")); // an update keeps the original arrival
    let keys: Vec<&str> = st.event_order.iter().map(|(_, _, k)| k.as_str()).collect();
    assert_eq!(keys, vec!["b", "a"]);
    let kb = ("h".to_string(), "transfer".to_string(), "b".to_string());
    assert_eq!(st.event_frames[&kb], 3);
}

fn ev(kind: &str, key: &str, state: &str, detail: Option<&str>) -> StreamLine {
    StreamLine::Event {
        host: "h".into(),
        kind: kind.into(),
        key: Some(key.into()),
        data: serde_json::json!({"state": state, "detail": detail}),
    }
}

#[test]
fn a_failed_status_event_makes_the_run_failed_even_when_ansible_exits_zero() {
    for (kind, state) in [
        ("step", "failed"),
        ("transfer", "missing"),
        ("transfer", "refused"),
        ("transfer", "failed"),
        ("node", "down"),
    ] {
        let mut st = RunState::default();
        st.apply(ev(kind, "k", state, Some("boom")));
        assert!(st.any_failed(), "{kind}/{state} must count as a failure");
        assert_eq!(st.event_failures().len(), 1);
        assert_eq!(st.event_failures()[0].detail, "boom");
    }
    for (kind, state) in [
        ("step", "ok"),
        ("step", "running"),
        ("transfer", "done"),
        ("node", "up"),
    ] {
        let mut st = RunState::default();
        st.apply(ev(kind, "k", state, None));
        assert!(!st.any_failed(), "{kind}/{state} is not a failure");
    }
    let mut st = RunState::default();
    st.apply(ev("step", "k", "failed", Some("old")));
    st.apply(ev("step", "k", "ok", None));
    assert!(
        !st.any_failed(),
        "a link that recovered later is not failed (last event wins)"
    );
}
