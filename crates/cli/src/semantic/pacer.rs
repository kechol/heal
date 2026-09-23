//! Client-side mirror of the server's input-token bucket.
//!
//! The endpoint limits input tokens, not requests, and answers an empty
//! bucket with a bare 429 (no `retry-after`). Following the measurement in
//! mizchi/jev-lint (`src/jev.ts`, MIT), the client keeps its own bucket,
//! charges each request's estimate before sending, corrects it to the
//! server's count afterwards, and backs off by a quarter on every 429.

use std::time::{Duration, Instant};

/// Measured refill rate of the server bucket (tokens per second).
pub const DEFAULT_TOKENS_PER_SECOND: f64 = 200_000.0;
/// Started a little under the measured ~1.6M burst.
pub const DEFAULT_TOKEN_BURST: f64 = 1_200_000.0;
const MIN_TOKENS_PER_SECOND: f64 = 20_000.0;

#[derive(Debug)]
pub struct Pacer {
    rate: f64,
    burst: f64,
    level: f64,
    at: Instant,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new(
            DEFAULT_TOKENS_PER_SECOND,
            DEFAULT_TOKEN_BURST,
            Instant::now(),
        )
    }
}

impl Pacer {
    #[must_use]
    pub fn new(rate: f64, burst: f64, now: Instant) -> Self {
        Self {
            rate,
            burst,
            level: burst,
            at: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.level = (self.level + elapsed * self.rate).min(self.burst);
        self.at = now;
    }

    /// How long to wait before `tokens` can be paid for. Zero when the
    /// bucket already holds enough. Requests larger than the burst are
    /// capped at the burst so they are never blocked forever.
    pub fn delay(&mut self, tokens: f64, now: Instant) -> Duration {
        self.refill(now);
        let need = tokens.min(self.burst) - self.level;
        if need <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(need / self.rate)
        }
    }

    /// Charge the bucket. Callers wait out [`Self::delay`] first.
    pub fn charge(&mut self, tokens: f64) {
        self.level -= tokens;
    }

    /// The server counted differently from the estimate.
    pub fn settle(&mut self, estimated: f64, actual: f64) {
        self.level -= actual - estimated;
    }

    /// A 429: the mirror was optimistic. Empty it and slow down.
    pub fn throttled(&mut self, now: Instant) {
        self.refill(now);
        self.level = 0.0;
        self.rate = (self.rate * 0.75).max(MIN_TOKENS_PER_SECOND);
    }

    #[must_use]
    pub fn rate(&self) -> f64 {
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_bucket_pays_immediately_and_empty_bucket_waits() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1000.0, 2000.0, t0);
        assert_eq!(p.delay(1500.0, t0), Duration::ZERO);
        p.charge(1500.0);
        let wait = p.delay(1500.0, t0);
        assert!((wait.as_secs_f64() - 1.0).abs() < 1e-6);
        assert_eq!(p.delay(1500.0, t0 + Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn throttle_empties_and_slows_with_a_floor() {
        let t0 = Instant::now();
        let mut p = Pacer::new(30_000.0, 100_000.0, t0);
        p.throttled(t0);
        assert!((p.rate() - 22_500.0).abs() < 1e-6);
        p.throttled(t0);
        assert!((p.rate() - MIN_TOKENS_PER_SECOND).abs() < 1e-6);
        assert!(p.delay(1.0, t0) > Duration::ZERO);
    }

    #[test]
    fn oversized_request_is_capped_at_burst() {
        let t0 = Instant::now();
        let mut p = Pacer::new(1000.0, 2000.0, t0);
        assert_eq!(p.delay(10_000.0, t0), Duration::ZERO);
    }
}
