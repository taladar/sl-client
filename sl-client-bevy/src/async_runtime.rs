//! A small **shared multi-threaded tokio runtime** for any subsystem that needs
//! to drive genuinely-async (non-blocking) work from a Bevy app, which has no
//! async runtime of its own.
//!
//! Bevy's task pools ([`IoTaskPool`](bevy::tasks::IoTaskPool) et al.) are real
//! async executors, but they are *not* tokio reactors: a future built on tokio's
//! IO (anything using `reqwest`'s async client, `tokio::net`, timers, …) panics
//! with "no reactor running" if polled on them. This module owns one small tokio
//! runtime the whole crate can offload such work to: hand it a future with
//! [`run_on_shared_runtime`] and `.await` the result. Awaiting yields
//! (`Poll::Pending`) on the Bevy executor that polls the caller, so the caller's
//! IO-pool thread is freed while tokio drives the work on its own threads.
//!
//! The first user of this is the asset fetch layer ([`crate::async_http`]), which
//! previously blocked a whole `IoTaskPool` thread per in-flight download; but the
//! runtime is deliberately fetch-agnostic so later async needs (a WebRTC signaller,
//! a background upload, …) can share it rather than each spinning up their own.
//!
//! The runtime is built lazily and **falls back gracefully**: if it cannot be
//! built, [`run_on_shared_runtime`] returns `None` and the caller takes its own
//! non-async path, so a runtime that fails to start never wedges the viewer.

use tokio::runtime::{Builder, Runtime};

/// Worker threads for the shared runtime. A small fixed pool, not `num_cpus`:
/// tokio multiplexes many concurrent non-blocking sockets / timers over a handful
/// of threads, and a bigger pool would only steal cores from Bevy's own compute /
/// async-compute pools. Four leaves headroom for per-request CPU (TLS handshakes,
/// body copies) while comfortably servicing every asset-store gate's admitted
/// requests at once.
const SHARED_RUNTIME_THREADS: usize = 4;

/// The shared multi-threaded tokio runtime, or `None` if it could not be built
/// (callers then take their non-async fallback). Built lazily on first use.
static SHARED_RUNTIME: std::sync::LazyLock<Option<Runtime>> = std::sync::LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(SHARED_RUNTIME_THREADS)
        .thread_name("sl-async-worker")
        .enable_all()
        .build()
        .ok()
});

/// Run `future` to completion on the shared runtime, returning its output, or
/// `None` if the runtime is unavailable (never built) or the spawned task was
/// cancelled / panicked — in which case the caller falls back to a non-async
/// path.
///
/// The `.await` on the returned tokio `JoinHandle` yields (`Poll::Pending`) on
/// the Bevy executor that polls the caller, so that executor's thread is freed to
/// do other work while tokio drives this future's IO on its own threads.
pub(crate) async fn run_on_shared_runtime<F, T>(future: F) -> Option<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let runtime = SHARED_RUNTIME.as_ref()?;
    runtime.spawn(future).await.ok()
}

#[cfg(test)]
mod tests {
    use super::run_on_shared_runtime;
    use bevy::tasks::block_on;
    use pretty_assertions::assert_eq;

    /// A future handed to the shared runtime runs to completion and its result is
    /// delivered back to the caller — verifying the cross-executor offload
    /// (awaiting a tokio `JoinHandle` from the non-tokio caller) actually works,
    /// which is the whole mechanism the async fetchers rely on. Driven here with
    /// Bevy's `block_on`, standing in for the `IoTaskPool` task that awaits it.
    #[test]
    fn runs_a_future_on_the_shared_runtime() {
        let result = block_on(run_on_shared_runtime(async { 2_u32.saturating_add(2) }));
        assert_eq!(result, Some(4));
    }

    /// A tokio-dependent future (a timer, which needs the runtime's reactor) also
    /// completes through the offload — the caller's executor is not a tokio
    /// reactor, so this only works because the future runs *on* the shared
    /// runtime.
    #[test]
    fn drives_a_tokio_timer_through_the_offload() {
        let result = block_on(run_on_shared_runtime(async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            7_u32
        }));
        assert_eq!(result, Some(7));
    }
}
