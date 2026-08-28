//! Case isolation: a panicking or hung case fails only itself.
//!
//! The runner drives exactly one case per invocation, but that case runs against
//! a *live* grid with avatars logged in. If its body panics, an unwinding panic
//! would tear the process down before the record is written and before the
//! sessions are logged out — leaving a stale presence on the grid that the next
//! run's login then has to fight (see
//! [`context::login`](crate::context::login)'s "already logged in" retry). If its
//! body hangs — a reply that never arrives inside a loop with no overall bound —
//! the runner waits forever.
//!
//! [`run_isolated`] closes both holes: it polls the case body inside
//! [`std::panic::catch_unwind`] and under an overall
//! [`tokio::time::timeout`], so either failure mode becomes an ordinary
//! [`TestFailure`] the runner records and then logs out around.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::context::TestFailure;
use crate::registry::TestFuture;

/// The overall wall-clock budget a case body gets before the runner gives up on
/// it, unless the case ([`GridTest::timeout`](crate::registry::GridTest::timeout))
/// or the operator (`--timeout`) says otherwise.
///
/// Generous on purpose: the slowest cases in the suite budget four minutes for a
/// single transfer, and a live aditi region can be far slower than the local
/// OpenSim. This is a backstop against a *hang*, not a performance assertion.
pub const DEFAULT_CASE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Run a case body so that a panic or a hang fails that one case instead of the
/// whole run.
///
/// A panic in the body is caught and returned as [`TestFailure::Panic`]; a body
/// still running after `timeout` is dropped (cancelling it at its current await
/// point) and returned as [`TestFailure::Timeout`]. In both cases the caller
/// keeps its [`TestContext`](crate::context::TestContext) — the borrow the body
/// held ends here — so it can still write the record and log the sessions out.
///
/// # Errors
///
/// Returns whatever [`TestFailure`] the body itself returned, or
/// [`TestFailure::Panic`] / [`TestFailure::Timeout`] as described above.
pub async fn run_isolated(body: TestFuture<'_>, timeout: Duration) -> Result<(), TestFailure> {
    match tokio::time::timeout(timeout, CatchUnwind { inner: body }).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(payload)) => Err(TestFailure::Panic(panic_message(payload.as_ref()))),
        Err(_elapsed) => Err(TestFailure::Timeout(format!(
            "case body exceeded its overall {} s timeout and was cancelled",
            timeout.as_secs()
        ))),
    }
}

/// Render a caught panic payload as a message, recovering the common
/// `&'static str` and `String` payloads that `panic!` produces.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "case body panicked with a non-string payload".to_owned()
    }
}

/// A future that catches an unwinding panic from the wrapped future's `poll`.
///
/// Deliberately hand-rolled rather than pulling in `futures`'
/// `FutureExt::catch_unwind`: the wrapped future is always an already-boxed
/// [`TestFuture`], which is [`Unpin`], so the projection needs no `unsafe`.
struct CatchUnwind<F> {
    /// The wrapped future.
    inner: F,
}

impl<F> Future for CatchUnwind<F>
where
    F: Future + Unpin,
{
    /// The wrapped future's output, or the payload of the panic that escaped it.
    type Output = Result<F::Output, Box<dyn Any + Send>>;

    /// Poll the wrapped future inside [`std::panic::catch_unwind`].
    ///
    /// The assertion of unwind safety is the point of the type: a case body that
    /// panics may well leave its own state inconsistent, and the caller's
    /// contract is that the *harness* survives — the sessions are logged out and
    /// the run is recorded as a failure, not resumed.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = &mut self.get_mut().inner;
        match std::panic::catch_unwind(AssertUnwindSafe(|| Pin::new(inner).poll(cx))) {
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{DEFAULT_CASE_TIMEOUT, run_isolated};
    use crate::context::TestFailure;

    /// A body that completes normally passes its own result through untouched.
    #[tokio::test]
    async fn passes_a_normal_outcome_through() {
        let ok = run_isolated(Box::pin(async { Ok(()) }), DEFAULT_CASE_TIMEOUT).await;
        assert!(ok.is_ok(), "a body that returned Ok was reported as {ok:?}");

        let failed = run_isolated(
            Box::pin(async { Err(TestFailure::Assertion("nope".to_owned())) }),
            DEFAULT_CASE_TIMEOUT,
        )
        .await;
        assert!(
            matches!(failed, Err(TestFailure::Assertion(ref message)) if message == "nope"),
            "the body's own failure was not passed through: {failed:?}"
        );
    }

    /// A panicking body becomes a [`TestFailure::Panic`] carrying its message —
    /// for both payload kinds a panic produces, and whether the panic escapes
    /// the first `poll` or a later one (after an await point).
    ///
    /// The panics are raised with [`std::panic::resume_unwind`] rather than
    /// `panic!`: it unwinds exactly the same way (which is what `run_isolated`
    /// catches), it can carry either payload type verbatim, and it neither trips
    /// the workspace's `clippy::panic` deny nor prints a panic block over the
    /// test output.
    #[tokio::test]
    async fn catches_a_panicking_body() {
        let literal = run_isolated(
            Box::pin(async {
                std::panic::resume_unwind(Box::new("static payload"));
            }),
            DEFAULT_CASE_TIMEOUT,
        )
        .await;
        let owned = run_isolated(
            Box::pin(async {
                std::panic::resume_unwind(Box::new(String::from("owned payload")));
            }),
            DEFAULT_CASE_TIMEOUT,
        )
        .await;
        let late = run_isolated(
            Box::pin(async {
                tokio::task::yield_now().await;
                std::panic::resume_unwind(Box::new("late payload"));
            }),
            DEFAULT_CASE_TIMEOUT,
        )
        .await;

        assert!(
            matches!(literal, Err(TestFailure::Panic(ref message)) if message == "static payload"),
            "a `&'static str` panic payload was not recovered: {literal:?}"
        );
        assert!(
            matches!(owned, Err(TestFailure::Panic(ref message)) if message == "owned payload"),
            "a `String` panic payload was not recovered: {owned:?}"
        );
        assert!(
            matches!(late, Err(TestFailure::Panic(ref message)) if message == "late payload"),
            "a panic after an await point was not caught: {late:?}"
        );
    }

    /// A body that never completes is cancelled at the overall timeout and
    /// reported as one, rather than hanging the runner.
    #[tokio::test(start_paused = true)]
    async fn times_a_hung_body_out() {
        let outcome = run_isolated(
            Box::pin(async {
                std::future::pending::<()>().await;
                Ok(())
            }),
            Duration::from_secs(90),
        )
        .await;
        assert!(
            matches!(outcome, Err(TestFailure::Timeout(ref message)) if message.contains("90 s")),
            "a hung body was not cancelled at its overall timeout: {outcome:?}"
        );
    }
}
