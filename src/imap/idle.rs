//! Live updates: while any browser of a user holds `/api/events` open, a second IMAP
//! connection for that user sits in IDLE on INBOX and every change is broadcast to all of
//! the user's listeners (keyed like the pool, by `Auth::ukey`).

use std::{sync::Arc, time::Duration};

use async_imap::extensions::idle::IdleResponse;
use dashmap::DashMap;
use tokio::sync::broadcast;

use super::Pool;
use crate::{
    error::{AppError, AppResult},
    session::Auth,
    types::LiveEvent,
};

/// How long one IDLE runs before it is renewed — also how quickly a watcher with no
/// remaining listeners notices and shuts down.
const IDLE_ROUND: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Live {
    channels: DashMap<String, broadcast::Sender<LiveEvent>>,
}

impl Live {
    pub fn subscribe(
        self: &Arc<Self>,
        pool: Arc<Pool>,
        auth: Auth,
    ) -> broadcast::Receiver<LiveEvent> {
        use dashmap::mapref::entry::Entry;
        match self.channels.entry(auth.ukey.clone()) {
            Entry::Occupied(e) => e.get().subscribe(),
            Entry::Vacant(e) => {
                let (tx, rx) = broadcast::channel(32);
                e.insert(tx.clone());
                tokio::spawn(watch(self.clone(), pool, auth, tx));
                rx
            }
        }
    }

    /// Ends the watcher for a session (sign-out). It exits after its current IDLE round.
    pub fn stop(&self, sid: &str) {
        self.channels.remove(sid);
    }

    fn wanted(&self, sid: &str, tx: &broadcast::Sender<LiveEvent>) -> bool {
        tx.receiver_count() > 0 && self.channels.get(sid).is_some_and(|c| c.same_channel(tx))
    }
}

async fn watch(live: Arc<Live>, pool: Arc<Pool>, auth: Auth, tx: broadcast::Sender<LiveEvent>) {
    let mut backoff = Duration::from_secs(1);
    while live.wanted(&auth.ukey, &tx) {
        match idle_inbox(&live, &pool, &auth, &tx).await {
            Ok(()) => break,
            Err(AppError::BadLogin) => break, // password changed; the API will 401 too
            Err(e) => {
                tracing::warn!(error = %e, "idle watcher failed; retrying");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
    live.channels
        .remove_if(&auth.ukey, |_, c| c.same_channel(&tx));
}

async fn idle_inbox(
    live: &Live,
    pool: &Pool,
    auth: &Auth,
    tx: &broadcast::Sender<LiveEvent>,
) -> AppResult<()> {
    let mut session = pool.connect(&auth.email, &auth.password).await?;
    session.select("INBOX").await?;
    loop {
        let mut handle = session.idle();
        handle.init().await?;
        let (wait, _stop) = handle.wait_with_timeout(IDLE_ROUND);
        let outcome = wait.await?;
        session = handle.done().await?;
        if let IdleResponse::NewData(_) = outcome {
            let _ = tx.send(LiveEvent {
                folder: "INBOX".to_owned(),
            });
        }
        if !live.wanted(&auth.ukey, tx) {
            let _ = session.logout().await;
            return Ok(());
        }
    }
}
