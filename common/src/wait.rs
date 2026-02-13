use anyhow::{Result, bail};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;

const MAX_WAIT_ITERATIONS: usize = 50;
const DEFAULT_CAP: Duration = Duration::from_secs(10);

pub async fn wait_with_interrupt(
    cancel: &CancellationToken,
    n: usize,
    interrupt: &mut Receiver<()>,
) -> Result<()> {
    wait_with_backoff_interrupt(cancel, n, interrupt).await
}

pub async fn wait(cancel: &CancellationToken, n: usize) -> Result<()> {
    wait_with_backoff(cancel, n, DEFAULT_CAP).await
}

/// Exponential backoff w/ "full jitter":
/// sleep for a random duration in [0, min(cap, base * 2^attempt)].
///
/// This tends to behave well under contention and avoids lockstep retries.
pub async fn wait_with_backoff(cancel: &CancellationToken, n: usize, cap: Duration) -> Result<()> {
    let n = n.clamp(1, MAX_WAIT_ITERATIONS);

    // Tune these as appropriate for your system.
    let base = Duration::from_millis(250);

    for attempt in 0..n {
        let delay = backoff_full_jitter(base, cap, attempt);

        tokio::select! {
            _ = cancel.cancelled() => bail!("Context cancelled"),
            _ = tokio::time::sleep(delay) => {}
        }
    }

    Ok(())
}

async fn wait_with_backoff_interrupt(
    cancel: &CancellationToken,
    n: usize,
    interrupt: &mut Receiver<()>,
) -> Result<()> {
    let n = n.clamp(1, MAX_WAIT_ITERATIONS);

    // Tune these as appropriate for your system.
    let base = Duration::from_millis(500);
    let cap = Duration::from_secs(10);

    for attempt in 0..n {
        let delay = backoff_full_jitter(base, cap, attempt);

        tokio::select! {
            _ = cancel.cancelled() => bail!("Context cancelled"),
            _ = tokio::time::sleep(delay) => {}
            _ = interrupt.recv() => {
                // Received interrupt signal, exit early
                return Ok(());
            }
        }
    }

    Ok(())
}

pub fn backoff_full_jitter(base: Duration, cap: Duration, attempt: usize) -> Duration {
    // Exponential term: base * 2^attempt, capped.
    // Use millis math to avoid Duration overflow footguns.
    let base_ms = base.as_millis() as u64;
    let cap_ms = cap.as_millis() as u64;

    // 2^attempt, saturating if attempt is huge.
    let pow = if attempt >= 63 {
        u64::MAX
    } else {
        1u64 << attempt
    };

    let exp_ms = base_ms.saturating_mul(pow);
    let upper = exp_ms.min(cap_ms).max(1); // avoid 0ms upper bound

    // Full jitter: uniform random in [0, upper]
    let jitter_ms = rand::random_range(0..=upper);
    Duration::from_millis(jitter_ms)
}
