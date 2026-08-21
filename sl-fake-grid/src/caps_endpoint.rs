//! The CAPS side of the HTTP service: routing one collected request into
//! [`SimCaps`] / the asset caps, including the `EventQueueGet` long-poll
//! hold the sans-I/O core deliberately leaves to its runtime.

use std::time::{Duration, Instant};

use sl_proto::{CapsDispatch, CapsRequest, CapsResponse};

use crate::driver::{SharedSim, SimState};

/// Dispatches one CAPS request against a session, holding an empty
/// `EventQueueGet` poll for up to `eq_hold` before answering the 502 the
/// client reads as "nothing yet, re-poll".
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
    let deadline = Instant::now().checked_add(eq_hold);
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
                // Session-free binary asset serving; no sim state touched.
                return guard.caps.assets().dispatch(&guard.assets, &request);
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
                let expired = match deadline {
                    Some(deadline) => {
                        tokio::select! {
                            () = shared.eq_notify.notified() => false,
                            () = tokio::time::sleep_until(
                                tokio::time::Instant::from_std(deadline),
                            ) => true,
                        }
                    }
                    // An unbounded hold: wait for the wakeup alone.
                    None => {
                        shared.eq_notify.notified().await;
                        false
                    }
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
