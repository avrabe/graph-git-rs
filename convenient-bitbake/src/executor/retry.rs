// ! Task execution retry logic with exponential backoff
//!
//! Handles transient failures in task execution with intelligent retry policies.

use super::types::{ExecutionError, ExecutionResult, TaskOutput};
use std::time::Duration;

/// Retry policy for task execution
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: usize,

    /// Initial backoff duration
    pub initial_backoff: Duration,

    /// Maximum backoff duration (to prevent excessive waits)
    pub max_backoff: Duration,

    /// Backoff multiplier (e.g., 2.0 for exponential backoff)
    pub backoff_multiplier: f64,

    /// Whether to retry on specific error types
    pub retry_on_timeout: bool,
    pub retry_on_io_error: bool,
    pub retry_on_sandbox_error: bool,
    pub retry_on_task_failure: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
            retry_on_io_error: true,
            retry_on_sandbox_error: false,  // Sandbox errors usually aren't transient
            retry_on_task_failure: false,   // Task failures usually aren't transient
        }
    }
}

impl RetryPolicy {
    /// No retries (fail fast)
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            ..Default::default()
        }
    }

    /// Conservative retry (safe for most tasks)
    pub fn conservative() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
            retry_on_io_error: true,
            retry_on_sandbox_error: false,
            retry_on_task_failure: false,
        }
    }

    /// Aggressive retry (for flaky tasks like network fetches)
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(120),
            backoff_multiplier: 2.0,
            retry_on_timeout: true,
            retry_on_io_error: true,
            retry_on_sandbox_error: true,   // Retry even sandbox errors
            retry_on_task_failure: false,
        }
    }

    /// Check if an error should be retried
    pub fn should_retry(&self, error: &ExecutionError) -> bool {
        match error {
            ExecutionError::Timeout(_) => self.retry_on_timeout,
            ExecutionError::IoError(_) => self.retry_on_io_error,
            ExecutionError::SandboxError(_) => self.retry_on_sandbox_error,
            ExecutionError::TaskFailed(_) => self.retry_on_task_failure,
            ExecutionError::CacheError(_) => self.retry_on_io_error,  // Treat as transient
            _ => false,  // Don't retry other errors
        }
    }

    /// Calculate backoff duration for attempt number (0-indexed)
    pub fn backoff_duration(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::from_secs(0);
        }

        let base_millis = self.initial_backoff.as_millis() as f64;
        let multiplier = self.backoff_multiplier.powi((attempt - 1) as i32);
        let backoff_millis = base_millis * multiplier;

        // Cap at max_backoff
        let backoff = Duration::from_millis(backoff_millis as u64);
        if backoff > self.max_backoff {
            self.max_backoff
        } else {
            backoff
        }
    }
}

/// Execute a task with retry logic
pub async fn execute_with_retry<F, Fut>(
    policy: &RetryPolicy,
    task_name: &str,
    mut execute_fn: F,
) -> ExecutionResult<TaskOutput>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ExecutionResult<TaskOutput>>,
{
    let mut last_error = None;

    for attempt in 0..policy.max_attempts {
        if attempt > 0 {
            let backoff = policy.backoff_duration(attempt);
            tracing::warn!(
                "Task '{}' failed, retrying in {:?} (attempt {}/{})",
                task_name,
                backoff,
                attempt + 1,
                policy.max_attempts
            );
            tokio::time::sleep(backoff).await;
        }

        match execute_fn().await {
            Ok(output) => {
                if attempt > 0 {
                    tracing::info!(
                        "Task '{}' succeeded on attempt {}/{}",
                        task_name,
                        attempt + 1,
                        policy.max_attempts
                    );
                }
                return Ok(output);
            }
            Err(error) => {
                tracing::debug!(
                    "Task '{}' failed on attempt {}/{}: {}",
                    task_name,
                    attempt + 1,
                    policy.max_attempts,
                    error
                );

                // Check if we should retry this error
                if !policy.should_retry(&error) {
                    tracing::warn!(
                        "Task '{}' failed with non-retryable error: {}",
                        task_name,
                        error
                    );
                    return Err(error);
                }

                last_error = Some(error);
            }
        }
    }

    // All attempts exhausted
    let error = last_error.unwrap();
    tracing::error!(
        "Task '{}' failed after {} attempts: {}",
        task_name,
        policy.max_attempts,
        error
    );
    Err(error)
}

/// Synchronous version of execute_with_retry
pub fn execute_with_retry_sync<F>(
    policy: &RetryPolicy,
    task_name: &str,
    mut execute_fn: F,
) -> ExecutionResult<TaskOutput>
where
    F: FnMut() -> ExecutionResult<TaskOutput>,
{
    let mut last_error = None;

    for attempt in 0..policy.max_attempts {
        if attempt > 0 {
            let backoff = policy.backoff_duration(attempt);
            tracing::warn!(
                "Task '{}' failed, retrying in {:?} (attempt {}/{})",
                task_name,
                backoff,
                attempt + 1,
                policy.max_attempts
            );
            std::thread::sleep(backoff);
        }

        match execute_fn() {
            Ok(output) => {
                if attempt > 0 {
                    tracing::info!(
                        "Task '{}' succeeded on attempt {}/{}",
                        task_name,
                        attempt + 1,
                        policy.max_attempts
                    );
                }
                return Ok(output);
            }
            Err(error) => {
                tracing::debug!(
                    "Task '{}' failed on attempt {}/{}: {}",
                    task_name,
                    attempt + 1,
                    policy.max_attempts,
                    error
                );

                // Check if we should retry this error
                if !policy.should_retry(&error) {
                    tracing::warn!(
                        "Task '{}' failed with non-retryable error: {}",
                        task_name,
                        error
                    );
                    return Err(error);
                }

                last_error = Some(error);
            }
        }
    }

    // All attempts exhausted
    let error = last_error.unwrap();
    tracing::error!(
        "Task '{}' failed after {} attempts: {}",
        task_name,
        policy.max_attempts,
        error
    );
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::ContentHash;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_backoff_calculation() {
        let policy = RetryPolicy::default();

        assert_eq!(policy.backoff_duration(0), Duration::from_secs(0));
        assert_eq!(policy.backoff_duration(1), Duration::from_secs(1));
        assert_eq!(policy.backoff_duration(2), Duration::from_secs(2));
        assert_eq!(policy.backoff_duration(3), Duration::from_secs(4));
        assert_eq!(policy.backoff_duration(4), Duration::from_secs(8));
    }

    #[test]
    fn test_backoff_cap() {
        let policy = RetryPolicy {
            max_backoff: Duration::from_secs(10),
            ..Default::default()
        };

        // Should cap at max_backoff
        assert!(policy.backoff_duration(10) <= Duration::from_secs(10));
    }

    #[test]
    fn test_should_retry() {
        let policy = RetryPolicy::default();

        assert!(policy.should_retry(&ExecutionError::Timeout(30)));
        assert!(policy.should_retry(&ExecutionError::IoError(
            std::io::Error::new(std::io::ErrorKind::Other, "test")
        )));
        assert!(!policy.should_retry(&ExecutionError::TaskFailed(1)));
        assert!(!policy.should_retry(&ExecutionError::SandboxError("test".to_string())));
    }

    #[test]
    fn test_no_retry_policy() {
        let policy = RetryPolicy::no_retry();
        assert_eq!(policy.max_attempts, 1);
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let policy = RetryPolicy::default();
        let attempt = Arc::new(AtomicUsize::new(0));
        let attempt_clone = Arc::clone(&attempt);

        let result = execute_with_retry(&policy, "test", move || {
            let attempt = Arc::clone(&attempt_clone);
            async move {
                let count = attempt.fetch_add(1, Ordering::SeqCst) + 1;
                if count < 2 {
                    Err(ExecutionError::Timeout(30))
                } else {
                    Ok(TaskOutput {
                        signature: ContentHash::from_bytes(b"test"),
                        output_files: Default::default(),
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 0,
                        duration_ms: 100,
                    })
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(attempt.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_fails_after_max_attempts() {
        let policy = RetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(1),
            ..Default::default()
        };
        let attempt = Arc::new(AtomicUsize::new(0));
        let attempt_clone = Arc::clone(&attempt);

        let result = execute_with_retry(&policy, "test", move || {
            let attempt = Arc::clone(&attempt_clone);
            async move {
                attempt.fetch_add(1, Ordering::SeqCst);
                Err::<TaskOutput, _>(ExecutionError::Timeout(30))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempt.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_non_retryable_error_fails_immediately() {
        let policy = RetryPolicy::default();
        let attempt = Arc::new(AtomicUsize::new(0));
        let attempt_clone = Arc::clone(&attempt);

        let result = execute_with_retry(&policy, "test", move || {
            let attempt = Arc::clone(&attempt_clone);
            async move {
                attempt.fetch_add(1, Ordering::SeqCst);
                Err::<TaskOutput, _>(ExecutionError::TaskFailed(1))
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempt.load(Ordering::SeqCst), 1);  // Should fail immediately
    }
}
