//! Turn SIGINT, SIGTERM and SIGHUP into a cancellation, so a child Ansible is never orphaned
//! (it runs in its own process group and would not get the terminal's Ctrl-C).

use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;

/// Cancel `token` when the process receives SIGINT, SIGTERM or SIGHUP.
pub fn cancel_on_signals(token: CancellationToken) {
    let kinds = [
        SignalKind::interrupt(),
        SignalKind::terminate(),
        SignalKind::hangup(),
    ];
    let mut streams = Vec::new();
    for k in kinds {
        if let Ok(s) = signal(k) {
            streams.push(s);
        }
    }
    if streams.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let waits = streams
            .into_iter()
            .map(|mut s| Box::pin(async move { s.recv().await }));
        futures_util::future::select_all(waits).await;
        token.cancel();
    });
}
