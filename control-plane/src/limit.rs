//! Token buckets, keyed by address or box: so many a second, with a burst.

use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Most keys kept. When full, the oldest are forgotten, which only ever lets
/// someone through early.
const MAX_KEYS: usize = 200_000;

/// A rate limit for many keys.
#[derive(Debug)]
pub struct Limiter<K> {
    per_second: f64,
    burst: f64,
    buckets: Mutex<HashMap<K, (f64, Instant)>>,
}

impl<K: Hash + Eq + Clone> Limiter<K> {
    /// `per_second` on average, up to `burst` at once.
    pub fn new(per_second: f64, burst: u32) -> Self {
        Self {
            per_second,
            burst: f64::from(burst.max(1)),
            buckets: Mutex::default(),
        }
    }

    /// One a `period`, up to `burst` at once.
    pub fn per(period: Duration, burst: u32) -> Self {
        Self::new(1.0 / period.as_secs_f64().max(0.001), burst)
    }

    /// Takes one token for `key`. `false` when there's none left.
    pub fn allow(&self, key: &K) -> bool {
        self.allow_at(key, Instant::now())
    }

    fn allow_at(&self, key: &K, now: Instant) -> bool {
        let Ok(mut b) = self.buckets.lock() else {
            return true;
        };
        if b.len() >= MAX_KEYS && !b.contains_key(key) {
            // Forget buckets that have filled up again; they'd allow anyway.
            let full_after = Duration::from_secs_f64(self.burst / self.per_second);
            b.retain(|_, (_, at)| now.duration_since(*at) < full_after);
            if b.len() >= MAX_KEYS {
                b.clear();
            }
        }
        let (tokens, at) = b.entry(key.clone()).or_insert((self.burst, now));
        let refill = now.duration_since(*at).as_secs_f64() * self.per_second;
        *tokens = (*tokens + refill).min(self.burst);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_then_refills() {
        let l = Limiter::new(2.0, 3);
        let t = Instant::now();
        assert!(l.allow_at(&"a", t));
        assert!(l.allow_at(&"a", t));
        assert!(l.allow_at(&"a", t));
        assert!(!l.allow_at(&"a", t));
        assert!(l.allow_at(&"b", t), "keys are separate");
        assert!(l.allow_at(&"a", t + Duration::from_millis(500)));
        assert!(!l.allow_at(&"a", t + Duration::from_millis(500)));
    }
}
