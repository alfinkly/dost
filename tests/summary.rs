use dost::stream::{RunState, StreamLine};
use dost::summary::{default_summary, render_done, summary_for};

fn state() -> RunState {
    let mut st = RunState::default();
    for (host, changed, failed) in [("a", true, false), ("b", false, true)] {
        st.apply(StreamLine::TaskResult {
            host: host.into(),
            task: "t".into(),
            changed,
            failed,
            skipped: false,
            unreachable: false,
            msg: None,
            diff: None,
        });
    }
    st
}

#[test]
fn done_template_lists_hosts() {
    let out = render_done("Perimeter on {{ hosts | join(', ') }}", &state()).unwrap();
    assert_eq!(out, "Perimeter on a, b");
}

#[test]
fn done_template_can_read_events_by_kind_and_host() {
    let mut st = state();
    st.apply(StreamLine::Event {
        host: "a".into(),
        kind: "ports".into(),
        key: None,
        data: serde_json::json!({"state": {"22": "on"}}),
    });
    let out = render_done("{{ events.ports.a.state['22'] }}", &st).unwrap();
    assert_eq!(out, "on");
}

#[test]
fn done_template_error_falls_back_to_default() {
    let st = state();
    assert!(render_done("{{ unclosed", &st).is_err());
    let text = summary_for(Some("{{ unclosed"), &st);
    assert_eq!(text, default_summary(&st));
}

#[test]
fn default_summary_counts_changed_and_failed() {
    let text = default_summary(&state());
    assert!(text.contains("2 hosts"), "{text}");
    assert!(text.contains("1 changed"), "{text}");
    assert!(text.contains("1 failed"), "{text}");
}
