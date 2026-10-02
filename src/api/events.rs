use std::{convert::Infallible, time::Duration};

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::{Stream, stream};
use tokio::sync::broadcast::error::RecvError;

use crate::{AppState, session::Auth};

/// Server-sent events: `change` with `{"folder": ...}` whenever the mailbox changes.
/// The gateway cuts long requests after 300s; EventSource reconnects by itself.
pub async fn events(
    State(st): State<AppState>,
    auth: Auth,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = st.live.subscribe(st.pool.clone(), auth);
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let event = Event::default()
                        .event("change")
                        .json_data(&ev)
                        .unwrap_or_default();
                    return Some((Ok(event), rx));
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}
