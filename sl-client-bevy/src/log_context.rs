//! Work handed to another thread runs in the **tracing context** of the code
//! that handed it over: the subscriber that was the spawning thread's default,
//! and the span that was current there.
//!
//! A thread starts with no span and the process's global subscriber, so
//! without this every line the session's network thread, a capability request
//! or a shared-runtime task logs is anonymous. A process running several
//! viewer Apps (the automation tier's in-process backend) updates each inside
//! a span of its own, and Bevy's task pools carry that span to the systems
//! (the Bevy fork's `bevy_tasks`); these helpers carry it the rest of the way,
//! onto the threads this crate starts itself. A test that captures logs with a
//! scoped subscriber sees them too.
//!
//! The context is captured when the work is **handed over**, so a thread
//! spawned from a thread that was itself spawned here inherits transitively.

use std::thread::JoinHandle;

use tracing::instrument::{Instrument as _, Instrumented, WithDispatch, WithSubscriber as _};

/// Spawn `work` on a new thread, in the calling thread's tracing context — the
/// drop-in for `std::thread::spawn`.
pub fn spawn_thread<F, T>(work: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let run = carrying_context(work);
    std::thread::spawn(run)
}

/// Spawn `work` on a new thread named `name`, in the calling thread's tracing
/// context — the drop-in for `std::thread::Builder::new().name(..).spawn(..)`.
///
/// # Errors
///
/// The operating system's refusal to start the thread.
pub fn spawn_named_thread<F, T>(name: &str, work: F) -> std::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let run = carrying_context(work);
    std::thread::Builder::new().name(name.to_owned()).spawn(run)
}

/// `work`, wrapped to run in the calling thread's tracing context wherever it
/// is later called.
pub fn carrying_context<F, T>(work: F) -> impl FnOnce() -> T + Send + 'static
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let span = tracing::Span::current();
    move || tracing::dispatcher::with_default(&dispatch, || span.in_scope(work))
}

/// `future`, wrapped to be polled in the calling thread's tracing context on
/// whichever executor runs it — for a task handed to a tokio runtime.
pub fn future_carrying_context<F: Future>(future: F) -> WithDispatch<Instrumented<F>> {
    future.in_current_span().with_current_subscriber()
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use std::sync::{Arc, Mutex, PoisonError};

    use pretty_assertions::assert_eq;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};
    use tracing_subscriber::registry::LookupSpan;

    use super::{future_carrying_context, spawn_named_thread, spawn_thread};

    /// Records, per event, the names of the spans it was inside.
    #[derive(Clone, Default)]
    struct Scopes(Arc<Mutex<Vec<Vec<String>>>>);

    impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for Scopes {
        fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
            let scope = context
                .event_scope(event)
                .map(|scope| scope.map(|span| span.name().to_owned()).collect())
                .unwrap_or_default();
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(scope);
        }
    }

    /// A thread or a runtime task logs to the subscriber, and inside the span,
    /// that the code handing it over had — not anonymously to the global one.
    #[test]
    fn handed_over_work_logs_in_the_spawners_context() {
        let scopes = Scopes::default();
        let subscriber = tracing_subscriber::registry().with(scopes.clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime");
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("viewer");
            let _entered = span.enter();
            spawn_thread(|| tracing::info!("plain"))
                .join()
                .expect("the thread ran");
            spawn_named_thread("named", || tracing::info!("named"))
                .expect("the thread started")
                .join()
                .expect("the thread ran");
            let task = runtime.spawn(future_carrying_context(async { tracing::info!("task") }));
            runtime.block_on(task).expect("the task ran");
        });
        let seen = scopes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(seen, vec![vec!["viewer".to_owned()]; 3]);
    }
}
