//! Supervisor Trees - Restart Policies and Tracking
//!
//! Provides fault-tolerance for actors through configurable restart policies.
//!
//! Features:
//! - Configurable restart limits within time windows
//! - Exponential backoff between restarts
//! - Per-actor restart tracking
//! - Circuit-breaker pattern when limits are exceeded

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Policy for restarting failed actors
#[derive(Debug, Clone)]
pub struct RestartPolicy {
    /// Maximum number of restarts within the time window
    pub max_restarts: u32,
    /// Time window for counting restarts
    pub window: Duration,
    /// Initial backoff duration between restarts
    pub backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 5,
            window: Duration::from_secs(60),
            backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
        }
    }
}

impl RestartPolicy {
    /// Create a new restart policy with custom values
    pub fn new(max_restarts: u32, window: Duration, backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_restarts,
            window,
            backoff,
            max_backoff,
        }
    }

    /// Create a policy that allows frequent restarts (for transient failures)
    pub fn lenient() -> Self {
        Self {
            max_restarts: 10,
            window: Duration::from_secs(60),
            backoff: Duration::from_millis(50),
            max_backoff: Duration::from_secs(2),
        }
    }

    /// Create a strict policy (for critical actors)
    pub fn strict() -> Self {
        Self {
            max_restarts: 3,
            window: Duration::from_secs(120),
            backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(10),
        }
    }
}

/// Tracks restart history for a single actor
#[derive(Debug)]
pub struct RestartTracker {
    /// Restart policy to apply
    policy: RestartPolicy,
    /// Timestamps of recent restarts (within window)
    restart_times: VecDeque<Instant>,
    /// Number of consecutive restarts (for backoff calculation)
    consecutive_restarts: u32,
}

impl RestartTracker {
    /// Create a new restart tracker with the given policy
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            restart_times: VecDeque::new(),
            consecutive_restarts: 0,
        }
    }

    /// Create a tracker with the default policy
    pub fn with_default_policy() -> Self {
        Self::new(RestartPolicy::default())
    }

    /// Record a restart attempt and check if it should be allowed.
    ///
    /// Returns `Some(backoff_duration)` if restart is allowed (with suggested backoff),
    /// or `None` if the restart limit has been exceeded.
    pub fn record_restart(&mut self) -> Option<Duration> {
        let now = Instant::now();

        // Remove restarts outside the time window
        let cutoff = now - self.policy.window;
        while let Some(front) = self.restart_times.front() {
            if *front < cutoff {
                self.restart_times.pop_front();
            } else {
                break;
            }
        }

        // Check if we've exceeded the limit
        if self.restart_times.len() >= self.policy.max_restarts as usize {
            return None;
        }

        // Record this restart
        self.restart_times.push_back(now);
        self.consecutive_restarts += 1;

        // Calculate backoff with exponential increase
        let backoff = self.calculate_backoff();
        Some(backoff)
    }

    /// Calculate the current backoff duration
    fn calculate_backoff(&self) -> Duration {
        let multiplier = 2u32.saturating_pow(self.consecutive_restarts.saturating_sub(1));
        let backoff_ms = self.policy.backoff.as_millis() as u64 * multiplier as u64;
        let backoff = Duration::from_millis(backoff_ms);
        backoff.min(self.policy.max_backoff)
    }

    /// Reset the consecutive restart counter (call after successful actor run)
    pub fn reset(&mut self) {
        self.consecutive_restarts = 0;
    }

    /// Get the number of restarts within the current window
    pub fn restart_count(&self) -> usize {
        self.restart_times.len()
    }

    /// Check if the restart limit has been exceeded
    pub fn is_exhausted(&self) -> bool {
        let now = Instant::now();
        let cutoff = now - self.policy.window;
        let recent_count = self
            .restart_times
            .iter()
            .filter(|t| **t >= cutoff)
            .count();
        recent_count >= self.policy.max_restarts as usize
    }

    /// Get the policy being used
    pub fn policy(&self) -> &RestartPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_default_policy() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.max_restarts, 5);
        assert_eq!(policy.window, Duration::from_secs(60));
    }

    #[test]
    fn test_restart_tracking() {
        let policy = RestartPolicy::new(
            3,
            Duration::from_secs(10),
            Duration::from_millis(10),
            Duration::from_millis(100),
        );
        let mut tracker = RestartTracker::new(policy);

        // First 3 restarts should be allowed
        assert!(tracker.record_restart().is_some());
        assert!(tracker.record_restart().is_some());
        assert!(tracker.record_restart().is_some());

        // 4th restart should be denied
        assert!(tracker.record_restart().is_none());
        assert!(tracker.is_exhausted());
    }

    #[test]
    fn test_exponential_backoff() {
        let policy = RestartPolicy::new(
            10,
            Duration::from_secs(60),
            Duration::from_millis(100),
            Duration::from_secs(5),
        );
        let mut tracker = RestartTracker::new(policy);

        let b1 = tracker.record_restart().unwrap();
        let b2 = tracker.record_restart().unwrap();
        let b3 = tracker.record_restart().unwrap();

        // Backoff should increase exponentially
        assert!(b2 > b1);
        assert!(b3 > b2);
    }

    #[test]
    fn test_window_expiry() {
        let policy = RestartPolicy::new(
            2,
            Duration::from_millis(100),
            Duration::from_millis(1),
            Duration::from_millis(10),
        );
        let mut tracker = RestartTracker::new(policy);

        // Use up the restart limit
        assert!(tracker.record_restart().is_some());
        assert!(tracker.record_restart().is_some());
        assert!(tracker.record_restart().is_none());

        // Wait for window to expire
        thread::sleep(Duration::from_millis(150));

        // Should be able to restart again
        assert!(tracker.record_restart().is_some());
    }

    #[test]
    fn test_reset() {
        let mut tracker = RestartTracker::with_default_policy();

        tracker.record_restart();
        tracker.record_restart();
        assert!(tracker.consecutive_restarts > 0);

        tracker.reset();
        assert_eq!(tracker.consecutive_restarts, 0);
    }
}
