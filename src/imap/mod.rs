//! IMAP access. One cached connection per web session (`Pool`), plus helpers in `ops`.

pub mod idle;
pub mod ops;
pub mod utf7;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use tokio::{
    net::TcpStream,
    sync::{Mutex, OwnedMutexGuard},
};
use tokio_rustls::{
    TlsConnector,
    rustls::{self, ClientConfig, RootCertStore, pki_types::ServerName},
};

use crate::{
    config::Config,
    error::{AppError, AppResult, UNREACHABLE},
    session::Auth,
};

pub type Stream = tokio_rustls::client::TlsStream<TcpStream>;
pub type Session = async_imap::Session<Stream>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// After a failed connect, further connects fail at once for this long instead of each
/// request waiting out its own timeout against a dead server.
const FAIL_FAST: Duration = Duration::from_secs(5);
/// Cached connections unused for this long are logged out.
const IDLE_EVICT: Duration = Duration::from_secs(10 * 60);

pub fn tls_config(accept_invalid_certs: bool) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions");
    let config = if accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(danger::AcceptAny(provider)))
            .with_no_client_auth()
    } else {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Arc::new(config)
}

/// Opens a TLS connection and logs in. A rejected login is `BadLogin`; anything else
/// (DNS, TCP, TLS, timeout) is a mail-server error.
pub async fn connect(
    cfg: &Config,
    tls: &Arc<ClientConfig>,
    user: &str,
    pass: &str,
) -> AppResult<Session> {
    let fut = async {
        let tcp = TcpStream::connect((cfg.imap_host.as_str(), cfg.imap_port))
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "imap connect");
                AppError::Unavailable(UNREACHABLE.into())
            })?;
        tcp.set_nodelay(true).ok();
        let name = ServerName::try_from(cfg.imap_host.clone())
            .map_err(|e| AppError::Mail(format!("server name: {e}")))?;
        let stream = TlsConnector::from(tls.clone())
            .connect(name, tcp)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "imap tls");
                AppError::Unavailable(UNREACHABLE.into())
            })?;
        let mut client = async_imap::Client::new(stream);
        client
            .read_response()
            .await
            .map_err(|_| AppError::Unavailable(UNREACHABLE.into()))?
            .ok_or_else(|| AppError::Unavailable(UNREACHABLE.into()))?;
        client
            .login(user, pass)
            .await
            .map_err(|(e, _)| login_error(e))
    };
    tokio::time::timeout(CONNECT_TIMEOUT.min(cfg.mail_timeout), fut)
        .await
        .map_err(|_| AppError::Unavailable("the mail server is not responding".into()))?
}

/// A refused LOGIN is only "wrong password" when the server says so. Dovecot answers
/// `NO [UNAVAILABLE]` when the user has too many connections from our IP, and that must not
/// be shown as a bad password or counted by the login rate limiter.
fn login_error(e: async_imap::error::Error) -> AppError {
    use async_imap::error::Error;
    match e {
        Error::No(msg) | Error::Bad(msg)
            if msg.contains("[UNAVAILABLE]")
                || msg.contains("[INUSE]")
                || msg.contains("[LIMIT]") =>
        {
            AppError::Mail(
                "the mail server has too many open connections for this account; \
                 try again in a minute"
                    .into(),
            )
        }
        Error::No(_) | Error::Bad(_) => AppError::BadLogin,
        e => AppError::Mail(e.to_string()),
    }
}

#[derive(Default)]
struct Slot {
    session: Option<Session>,
    last_used: Option<Instant>,
    /// Recent sort results; see `Conn::sorted`. Survives reconnects (it is keyed on the
    /// mailbox's own change counters, not on the connection).
    sorts: std::collections::VecDeque<SortEntry>,
}

const SORT_CACHE: usize = 16;

struct SortEntry {
    folder: String,
    criteria: String,
    validity: Option<u32>,
    modseq: u64,
    exists: u32,
    uids: Arc<Vec<u32>>,
}

/// One IMAP connection per user (`Auth::ukey`), serialised by a mutex and shared by all of
/// that user's web sessions. Connections are opened lazily and re-opened after any failure.
pub struct Pool {
    cfg: Arc<Config>,
    tls: Arc<ClientConfig>,
    slots: DashMap<String, Arc<Mutex<Slot>>>,
    /// Set after a failed connect; see FAIL_FAST.
    down_until: std::sync::Mutex<Option<Instant>>,
}

/// Exclusive use of a session's connection. Unless `release()` is called, dropping it
/// discards the connection: an early return, error or cancelled request may have left the
/// protocol mid-command, and reconnecting is cheaper than guessing.
pub struct Conn {
    guard: OwnedMutexGuard<Slot>,
    released: bool,
}

impl Conn {
    pub fn s(&mut self) -> &mut Session {
        self.guard
            .session
            .as_mut()
            .expect("Conn always holds a session")
    }

    pub fn release(mut self) {
        self.released = true;
    }

    /// Selects `folder` and returns its UIDs sorted newest-first for `criteria`, reusing the
    /// last result while the folder is unchanged: same UIDVALIDITY, HIGHESTMODSEQ and count.
    /// Paging and returning to a folder then skip the server-side sort and the transfer of
    /// every UID. Servers without CONDSTORE are never cached.
    pub async fn sorted(&mut self, folder: &str, criteria: &str) -> AppResult<Arc<Vec<u32>>> {
        let mb = ops::select(self.s(), folder).await?;
        let Some(modseq) = mb.highest_modseq else {
            return Ok(Arc::new(ops::sorted_uids(self.s(), criteria).await?));
        };
        let sorts = &mut self.guard.sorts;
        if let Some(e) = sorts.iter().find(|e| {
            e.folder == folder
                && e.criteria == criteria
                && e.validity == mb.uid_validity
                && e.modseq == modseq
                && e.exists == mb.exists
        }) {
            return Ok(e.uids.clone());
        }
        let uids = Arc::new(ops::sorted_uids(self.s(), criteria).await?);
        let sorts = &mut self.guard.sorts;
        sorts.retain(|e| !(e.folder == folder && e.criteria == criteria));
        if sorts.len() >= SORT_CACHE {
            sorts.pop_front();
        }
        sorts.push_back(SortEntry {
            folder: folder.to_owned(),
            criteria: criteria.to_owned(),
            validity: mb.uid_validity,
            modseq,
            exists: mb.exists,
            uids: uids.clone(),
        });
        Ok(uids)
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        if !self.released {
            self.guard.session = None;
        }
    }
}

impl Pool {
    pub fn new(cfg: Arc<Config>) -> Self {
        Self {
            tls: tls_config(cfg.tls_accept_invalid_certs),
            cfg,
            slots: DashMap::new(),
            down_until: std::sync::Mutex::new(None),
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub async fn connect(&self, user: &str, pass: &str) -> AppResult<Session> {
        if self
            .down_until
            .lock()
            .expect("not poisoned")
            .is_some_and(|t| Instant::now() < t)
        {
            return Err(AppError::Unavailable(UNREACHABLE.into()));
        }
        let r = connect(&self.cfg, &self.tls, user, pass).await;
        let mut down = self.down_until.lock().expect("not poisoned");
        match &r {
            Err(AppError::Unavailable(_)) => *down = Some(Instant::now() + FAIL_FAST),
            _ => *down = None,
        }
        r
    }

    /// Hands a freshly logged-in connection to the pool (used right after sign-in). If the
    /// user already has one, the new connection is closed instead.
    pub async fn put(&self, ukey: &str, mut session: Session) {
        let slot = self.slots.entry(ukey.to_owned()).or_default().clone();
        let mut g = slot.lock().await;
        if g.session.is_none() {
            g.session = Some(session);
            g.last_used = Some(Instant::now());
        } else {
            drop(g);
            let _ = session.logout().await;
        }
    }

    pub async fn get(&self, auth: &Auth) -> AppResult<Conn> {
        let slot = self.slots.entry(auth.ukey.clone()).or_default().clone();
        let mut guard = slot.lock_owned().await;
        if guard.session.is_none() {
            guard.session = Some(self.connect(&auth.email, &auth.password).await?);
        }
        guard.last_used = Some(Instant::now());
        Ok(Conn {
            guard,
            released: false,
        })
    }

    pub async fn remove(&self, ukey: &str) {
        if let Some((_, slot)) = self.slots.remove(ukey)
            && let Some(mut s) = slot.lock().await.session.take()
        {
            let _ = s.logout().await;
        }
    }

    /// Logs out connections nobody has used for a while. Busy slots are skipped.
    pub async fn reap(&self) {
        let stale: Vec<String> = self
            .slots
            .iter()
            .filter(|e| {
                e.value()
                    .try_lock()
                    .is_ok_and(|s| s.last_used.is_none_or(|t| t.elapsed() > IDLE_EVICT))
            })
            .map(|e| e.key().clone())
            .collect();
        for ukey in stale {
            self.remove(&ukey).await;
        }
    }
}

mod danger {
    use std::sync::Arc;

    use tokio_rustls::rustls::{
        DigitallySignedStruct, Error, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
        pki_types::{CertificateDer, ServerName, UnixTime},
    };

    /// Accepts any certificate. Only reachable with TLS_ACCEPT_INVALID_CERTS=1 (dev stack).
    #[derive(Debug)]
    pub struct AcceptAny(pub Arc<CryptoProvider>);

    impl ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_imap::error::Error;

    #[test]
    fn only_real_auth_failures_are_bad_logins() {
        assert!(matches!(
            login_error(Error::No(
                "[AUTHENTICATIONFAILED] Authentication failed.".into()
            )),
            AppError::BadLogin
        ));
        assert!(matches!(
            login_error(Error::No("Authentication failed.".into())),
            AppError::BadLogin
        ));
        assert!(matches!(
            login_error(Error::No(
                "[UNAVAILABLE] Maximum number of connections from user+IP exceeded".into()
            )),
            AppError::Mail(_)
        ));
    }
}
