//! CAPS subsystem lifecycle: seed/map fetch and EventQueueGet long-poll.

use crate::{Caps, EVENT_QUEUE_TIMEOUT};
use bevy::prelude::*;
use crossbeam_channel::{Sender, unbounded};
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    Llsd, REQUESTED_CAPABILITIES, Session, build_event_queue_request, build_seed_request,
    parse_event_queue_response, parse_seed_response,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The reserved `(message, body)` key a CAPS helper sends over the events
/// channel when its request failed before producing a reply. The driver
/// recognises the `\0caps-failure\0` prefix, logs it, and — when diagnostics are
/// enabled — surfaces a [`Diagnostic::ExpectedReplyMissing`](sl_proto::Diagnostic::ExpectedReplyMissing)
/// instead of passing it to
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event). The NUL
/// prefix cannot collide with a real capability / event-queue name.
pub(crate) const CAPS_FAILURE_PREFIX: &str = "\0caps-failure\0";

/// Reports that a CAPS request for `cap` failed before producing a reply,
/// sending the failure sentinel over `caps_tx`. Helpers call this in place of
/// silently swallowing a transport / parse error; the driver turns it into a
/// diagnostic.
pub(crate) fn report_caps_failure(caps_tx: &Sender<(String, Llsd)>, cap: &str) {
    caps_tx
        .send((format!("{CAPS_FAILURE_PREFIX}{cap}"), Llsd::Undef))
        .ok();
}

/// Starts the CAPS subsystem for the session's current seed capability: a
/// background thread that fetches the capability map (reported over `map_rx`)
/// then long-polls `EventQueueGet`. Returns `None` if no seed is known yet.
pub(crate) fn start_caps(session: &Session) -> Option<Caps> {
    let seed = session.seed_capability()?.to_owned();
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_events = events_tx.clone();
    std::thread::spawn(move || run_caps(seed, &thread_events, &map_tx, &thread_stop));
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        map_rx,
        map: HashMap::new(),
        stop,
    })
}

/// POSTs a neighbour region's seed capability (in a detached thread, result
/// ignored) so the simulator marks the agent's capabilities as sent and begins
/// streaming that region's scene to the child circuit.
pub(crate) fn post_neighbour_seed(seed_url: url::Url) {
    std::thread::spawn(move || {
        let Ok(http) = ReqwestBlockingClient::builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        else {
            return;
        };
        let _ignored = http
            .post(seed_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_seed_request(REQUESTED_CAPABILITIES))
            .send();
    });
}

/// Fetches the capability map from `seed_url` (reporting it over `map_tx`), then
/// long-polls the `EventQueueGet` capability, forwarding each decoded event to
/// `caps_tx` until `stop` is set, a receiver is dropped (e.g. on region change),
/// or a request fails fatally.
pub(crate) fn run_caps(
    seed_url: url::Url,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<HashMap<String, String>>,
    stop: &AtomicBool,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    let Ok(capabilities) = parse_seed_response(&text) else {
        return;
    };
    map_tx.send(capabilities.clone()).ok();
    let Some(event_queue_url) = capabilities.get("EventQueueGet").cloned() else {
        return;
    };

    let mut ack: Option<i32> = None;
    while !stop.load(Ordering::Relaxed) {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
        {
            Ok(response) => response,
            Err(_error) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).is_err() {
                return;
            }
        }
    }
}
