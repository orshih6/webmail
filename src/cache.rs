//! Parsed, sanitized messages, kept in memory so opening one again (or a prefetched one)
//! skips the IMAP fetch, MIME parse and HTML sanitizing.
//!
//! Safe because IMAP guarantees a message's *content* never changes for a given
//! (UIDVALIDITY, UID): the key includes both, plus the user and folder. Flags do change, so
//! they are never cached — callers fetch them fresh and overwrite. Bounded by a byte budget
//! (least recently used goes first) and a TTL, and a user's entries are dropped at sign-out.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::types::MessageDetail;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    /// `Auth::ukey`: entries are never shared between users.
    pub user: String,
    pub folder: String,
    pub validity: u32,
    pub uid: u32,
    /// Remote content allowed or not: two different sanitized renderings.
    pub images: bool,
}

struct Entry {
    detail: Arc<MessageDetail>,
    bytes: usize,
    at: Instant,
}

struct Inner {
    map: HashMap<Key, Entry>,
    /// Least recently used at the front. May hold keys already removed; skipped lazily.
    order: VecDeque<Key>,
    bytes: usize,
}

pub struct MessageCache {
    inner: Mutex<Inner>,
    budget: usize,
    ttl: Duration,
}

/// Rough heap size of a detail: what dominates is the HTML and text bodies.
fn weight(d: &MessageDetail) -> usize {
    let s = |o: &Option<String>| o.as_ref().map_or(0, String::len);
    512 + s(&d.html) + s(&d.text) + d.subject.len() + d.attachments.len() * 128
}

impl MessageCache {
    pub fn new(budget_bytes: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
            }),
            budget: budget_bytes,
            ttl,
        }
    }

    pub fn get(&self, key: &Key) -> Option<Arc<MessageDetail>> {
        let mut g = self.inner.lock().expect("not poisoned");
        let fresh = g.map.get(key).is_some_and(|e| e.at.elapsed() < self.ttl);
        if !fresh {
            if let Some(e) = g.map.remove(key) {
                g.bytes -= e.bytes;
            }
            return None;
        }
        // Touch: move to the back of the LRU order.
        g.order.retain(|k| k != key);
        g.order.push_back(key.clone());
        g.map.get(key).map(|e| e.detail.clone())
    }

    pub fn put(&self, key: Key, detail: Arc<MessageDetail>) {
        let bytes = weight(&detail);
        if bytes > self.budget / 4 {
            return; // one huge message must not flush everything else
        }
        let mut g = self.inner.lock().expect("not poisoned");
        if let Some(old) = g.map.remove(&key) {
            g.bytes -= old.bytes;
        }
        g.order.retain(|k| k != &key);
        while g.bytes + bytes > self.budget {
            let Some(oldest) = g.order.pop_front() else {
                break;
            };
            if let Some(e) = g.map.remove(&oldest) {
                g.bytes -= e.bytes;
            }
        }
        g.bytes += bytes;
        g.order.push_back(key.clone());
        g.map.insert(
            key,
            Entry {
                detail,
                bytes,
                at: Instant::now(),
            },
        );
    }

    /// Sign-out: nothing of this user's mail stays in memory.
    pub fn forget_user(&self, user: &str) {
        let mut g = self.inner.lock().expect("not poisoned");
        let gone: Vec<Key> = g.map.keys().filter(|k| k.user == user).cloned().collect();
        for k in gone {
            if let Some(e) = g.map.remove(&k) {
                g.bytes -= e.bytes;
            }
        }
        g.order.retain(|k| k.user != user);
    }

    /// Drops expired entries; run periodically.
    pub fn sweep(&self) {
        let mut g = self.inner.lock().expect("not poisoned");
        let ttl = self.ttl;
        let expired: Vec<Key> = g
            .map
            .iter()
            .filter(|(_, e)| e.at.elapsed() >= ttl)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &expired {
            if let Some(e) = g.map.remove(k) {
                g.bytes -= e.bytes;
            }
        }
        g.order.retain(|k| !expired.contains(k));
    }

    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.inner.lock().unwrap().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Flags;

    fn detail(body: usize) -> Arc<MessageDetail> {
        Arc::new(MessageDetail {
            folder: "INBOX".into(),
            uid: 1,
            subject: "s".into(),
            from: None,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            date: None,
            message_id: None,
            in_reply_to: None,
            references: vec![],
            flags: Flags::default(),
            html: None,
            text: Some("x".repeat(body)),
            remote_images_blocked: false,
            attachments: vec![],
        })
    }

    fn key(user: &str, uid: u32) -> Key {
        Key {
            user: user.into(),
            folder: "INBOX".into(),
            validity: 7,
            uid,
            images: false,
        }
    }

    #[test]
    fn hits_are_per_user_folder_validity_uid_and_rendering() {
        let c = MessageCache::new(1 << 20, Duration::from_secs(60));
        c.put(key("alice", 1), detail(10));
        assert!(c.get(&key("alice", 1)).is_some());
        assert!(c.get(&key("bob", 1)).is_none(), "never across users");
        assert!(
            c.get(&Key {
                validity: 8,
                ..key("alice", 1)
            })
            .is_none(),
            "new UIDVALIDITY"
        );
        assert!(
            c.get(&Key {
                images: true,
                ..key("alice", 1)
            })
            .is_none(),
            "other rendering"
        );
        assert!(
            c.get(&Key {
                folder: "Sent".into(),
                ..key("alice", 1)
            })
            .is_none()
        );
    }

    #[test]
    fn evicts_least_recently_used_within_budget() {
        let c = MessageCache::new(4 * 1613, Duration::from_secs(60)); // exactly 4 entries (512 + 1100 + 1 B each)
        for uid in 1..=4 {
            c.put(key("a", uid), detail(1100));
        }
        assert!(c.get(&key("a", 1)).is_some()); // touch 1: now 2 is the oldest
        c.put(key("a", 5), detail(1100));
        assert!(c.get(&key("a", 2)).is_none(), "LRU evicted");
        assert!(c.get(&key("a", 1)).is_some() && c.get(&key("a", 5)).is_some());
        assert!(c.bytes() <= 4 * 1613);
    }

    #[test]
    fn huge_messages_are_not_cached() {
        let c = MessageCache::new(10_000, Duration::from_secs(60));
        c.put(key("a", 1), detail(5_000));
        assert!(c.get(&key("a", 1)).is_none());
    }

    #[test]
    fn ttl_sweep_and_forget_user() {
        let c = MessageCache::new(1 << 20, Duration::from_millis(30));
        c.put(key("a", 1), detail(10));
        c.put(key("b", 1), detail(10));
        c.forget_user("a");
        assert!(c.get(&key("a", 1)).is_none());
        assert!(c.get(&key("b", 1)).is_some());
        std::thread::sleep(Duration::from_millis(40));
        c.sweep();
        assert!(c.get(&key("b", 1)).is_none());
        assert_eq!(c.bytes(), 0);
    }
}
