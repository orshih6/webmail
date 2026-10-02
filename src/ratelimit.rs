//! Failed-login limiter, in memory (single replica). Keys are per client IP and per account,
//! so neither spraying one account from many IPs nor many accounts from one IP gets far.

use std::time::{Duration, Instant};

use dashmap::DashMap;

const WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_FAILURES: u32 = 10;

#[derive(Default)]
pub struct LoginLimiter {
    failures: DashMap<String, (u32, Instant)>,
}

impl LoginLimiter {
    pub fn blocked(&self, key: &str) -> bool {
        self.failures
            .get(key)
            .is_some_and(|e| e.1.elapsed() < WINDOW && e.0 >= MAX_FAILURES)
    }

    pub fn fail(&self, key: &str) {
        let mut e = self
            .failures
            .entry(key.to_owned())
            .or_insert((0, Instant::now()));
        if e.1.elapsed() >= WINDOW {
            *e = (0, Instant::now());
        }
        e.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.failures.remove(key);
    }

    pub fn sweep(&self) {
        self.failures.retain(|_, e| e.1.elapsed() < WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_max_failures() {
        let l = LoginLimiter::default();
        for _ in 0..MAX_FAILURES - 1 {
            l.fail("ip:1");
        }
        assert!(!l.blocked("ip:1"));
        l.fail("ip:1");
        assert!(l.blocked("ip:1"));
        assert!(!l.blocked("ip:2"));
        l.clear("ip:1");
        assert!(!l.blocked("ip:1"));
    }
}
