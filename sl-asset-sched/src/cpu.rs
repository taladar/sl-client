//! The rayon bridge: running a CPU-bound task off the async caller's thread.
//!
//! Decoding and downsampling are CPU-bound and would block an async executor, so
//! a store runs them on the shared `rayon` pool via [`run_cpu`], bounded by a
//! semaphore so only so many run at once. The result travels back over a bounded
//! `async_channel` the caller awaits.

/// Runs a CPU-bound task on the rayon pool (off the caller's thread), bounded by
/// `permits`, and awaits its result. Returns `None` if the worker was lost (e.g.
/// a panic dropped the sender before it produced a result).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `run_cpu` reads clearly"
)]
pub async fn run_cpu<T, F>(permits: &async_lock::Semaphore, task: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = permits.acquire().await;
    let (sender, receiver) = async_channel::bounded(1);
    rayon::spawn(move || {
        let _sent = sender.send_blocking(task());
    });
    let result = receiver.recv().await.ok();
    drop(permit);
    result
}

#[cfg(test)]
mod tests {
    use super::run_cpu;

    #[test]
    fn run_cpu_returns_the_task_result() {
        pollster::block_on(async {
            let permits = async_lock::Semaphore::new(1);
            let doubled = run_cpu(&permits, || 21_u32.saturating_mul(2)).await;
            pretty_assertions::assert_eq!(doubled, Some(42));
        });
    }
}
