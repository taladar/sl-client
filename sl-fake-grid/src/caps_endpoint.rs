//! The CAPS side of the HTTP service: routing one collected request into
//! [`SimCaps`] / the asset caps, including the `EventQueueGet` long-poll
//! hold the sans-I/O core deliberately leaves to its runtime.

use std::time::Duration;

use sl_proto::{CapsDispatch, CapsRequest, CapsResponse};

use crate::driver::{SharedSim, SimState, sleep_until_opt};

/// Dispatches one CAPS request against a session, holding an empty
/// `EventQueueGet` poll for up to `eq_hold` before answering the 502 the
/// client reads as "nothing yet, re-poll". The hold also ends the moment the
/// grid shuts down, so teardown is never held hostage by a poll that would
/// otherwise sit for the whole `eq_hold`.
///
/// Asset-delivery caps (`GetTexture`, `GetMesh`, `ViewerAsset`, …) route to
/// the session-free asset surface against the scenario's asset store;
/// everything else goes through [`sl_proto::SimCaps::dispatch`] followed by
/// the driver's flush rule (a caps POST can queue transmits and events).
pub(crate) async fn dispatch_caps(
    shared: &SharedSim,
    eq_hold: Duration,
    method: &str,
    path: &str,
    query: Option<&str>,
    range: Option<&str>,
    body: &[u8],
) -> CapsResponse {
    // The hold deadline covers the whole poll, not each re-dispatch.
    let deadline = shared.now().checked_add(eq_hold);
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        let request = CapsRequest {
            method,
            path,
            query,
            range,
            body,
        };
        let dispatch = {
            let mut guard = shared.state.lock().await;
            if guard.caps.assets().handles_path(path) {
                // Session-free binary asset serving out of the **grid-wide**
                // store: no sim state touched, and no region's content is
                // unreachable from another region's capability. The read guard
                // never outlives this block, so no await ever happens under it.
                let store = guard.assets.read();
                return guard.caps.assets().dispatch(&*store, &request);
            }
            let SimState { sim, caps, .. } = &mut *guard;
            let dispatch = caps.dispatch(sim, &request);
            let outcome = shared.flush_locked(&mut guard);
            drop(guard);
            shared.finish_flush(outcome).await;
            dispatch
        };
        match dispatch {
            CapsDispatch::Response(response) => return response,
            CapsDispatch::EventQueueWouldBlock => {
                if *shutdown_rx.borrow_and_update() {
                    let guard = shared.state.lock().await;
                    return guard.caps.event_queue_timeout();
                }
                // A hold with no deadline waits for the wakeup (or the
                // shutdown) alone.
                let expired = tokio::select! {
                    () = shared.eq_notify.notified() => false,
                    () = sleep_until_opt(deadline) => true,
                    changed = shutdown_rx.changed() => changed.is_err() || *shutdown_rx.borrow(),
                };
                if expired {
                    let guard = shared.state.lock().await;
                    return guard.caps.event_queue_timeout();
                }
                // Woken: loop back and re-dispatch against the fresh queue.
            }
        }
    }
}
