use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Simple per-IP rate limiter for login attempts.
/// Allows `max_attempts` within `window` duration per IP address.
pub struct LoginRateLimiter {
    attempts: RwLock<HashMap<String, VecDeque<Instant>>>,
    max_attempts: usize,
    window: Duration,
}

impl LoginRateLimiter {
    pub fn new(max_attempts: usize, window: Duration) -> Self {
        Self {
            attempts: RwLock::new(HashMap::new()),
            max_attempts,
            window,
        }
    }

    /// Check if a login attempt from the given IP is allowed.
    /// Returns `Ok(())` if allowed, `Err` with remaining seconds if rate-limited.
    pub fn check(&self, ip: &str) -> Result<(), u64> {
        let mut map = match self.attempts.write() {
            Ok(m) => m,
            Err(_) => {
                tracing::error!(
                    limiter = std::any::type_name::<Self>(),
                    "Rate limiter lock poisoned; failing closed"
                );
                return Err(self.window.as_secs().max(1));
            }
        };
        let now = Instant::now();
        let entry = map.entry(ip.to_string()).or_insert_with(VecDeque::new);

        // Remove expired attempts
        while let Some(front) = entry.front() {
            if now.duration_since(*front) > self.window {
                entry.pop_front();
            } else {
                break;
            }
        }

        if entry.len() >= self.max_attempts {
            // Safety: len() >= max_attempts (>= 1), so the deque is non-empty.
            let oldest = entry
                .front()
                .expect("deque non-empty (guarded by len check)");
            let retry_after = self.window.as_secs() - now.duration_since(*oldest).as_secs();
            return Err(retry_after.max(1));
        }

        entry.push_back(now);
        Ok(())
    }

    /// Remove entries whose all timestamps have expired.
    /// Call periodically from a background task to prevent unbounded HashMap growth.
    pub fn evict_stale(&self) {
        if let Ok(mut map) = self.attempts.write() {
            let now = Instant::now();
            map.retain(|_, deque| {
                // Drain expired timestamps from the front
                while let Some(front) = deque.front() {
                    if now.duration_since(*front) > self.window {
                        deque.pop_front();
                    } else {
                        break;
                    }
                }
                !deque.is_empty()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rate_limiter_allows_within_limit() {
        let limiter = LoginRateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check("10.0.0.1").is_ok());
        assert!(limiter.check("10.0.0.1").is_ok());
        assert!(limiter.check("10.0.0.1").is_ok());
    }

    #[test]
    fn rate_limiter_blocks_when_exceeded() {
        let limiter = LoginRateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check("10.0.0.1").is_ok());
        assert!(limiter.check("10.0.0.1").is_ok());
        let result = limiter.check("10.0.0.1");
        assert!(result.is_err(), "Third attempt should be rejected");
        let retry_after = result.unwrap_err();
        assert!(
            retry_after >= 1,
            "retry_after should be at least 1 second, got {retry_after}"
        );
    }

    #[test]
    fn rate_limiter_isolates_ips() {
        let limiter = LoginRateLimiter::new(1, Duration::from_secs(60));
        assert!(limiter.check("10.0.0.1").is_ok());
        // Different IP should still be allowed
        assert!(limiter.check("10.0.0.2").is_ok());
        // First IP is now blocked
        assert!(limiter.check("10.0.0.1").is_err());
    }

    #[test]
    fn rate_limiter_fails_closed_on_poison() {
        let limiter = Arc::new(LoginRateLimiter::new(10, Duration::from_secs(60)));
        let limiter_for_thread = Arc::clone(&limiter);

        let _ = std::thread::spawn(move || {
            let _guard = limiter_for_thread.attempts.write().unwrap();
            panic!("poison rate limiter lock");
        })
        .join();

        let result = limiter.check("10.0.0.1");
        assert!(result.is_err(), "poisoned limiter must reject requests");
        assert_eq!(result.unwrap_err(), 60);
    }

    #[test]
    fn rate_limiter_expired_attempts_cleaned_up() {
        // Use a tiny window so attempts expire almost immediately
        let limiter = LoginRateLimiter::new(1, Duration::from_millis(1));
        assert!(limiter.check("10.0.0.1").is_ok());
        // Wait for the window to expire
        std::thread::sleep(Duration::from_millis(5));
        // Should be allowed again because the old attempt expired
        assert!(
            limiter.check("10.0.0.1").is_ok(),
            "Expired attempts should be cleaned up, allowing new ones"
        );
    }
}
