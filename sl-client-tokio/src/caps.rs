//! CAPS lifecycle: capability fetch, event-queue spawn/poll, task helpers.

use crate::IDLE_SLEEP;
use reqwest::Client as ReqwestClient;
use sl_proto::{
    CAP_SIMULATOR_FEATURES, Llsd, REQUESTED_CAPABILITIES, build_event_queue_request,
    build_seed_request, parse_event_queue_response, parse_seed_response,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// The reserved `(message, body)` key a CAPS helper sends over the events
/// channel when its request failed before producing a reply. The run loop
/// recognises the `\0caps-failure\0` prefix, logs it, and — when diagnostics are
/// enabled — surfaces a [`Diagnostic::ExpectedReplyMissing`](sl_proto::Diagnostic::ExpectedReplyMissing)
/// instead of passing it to
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event). The NUL
/// prefix cannot collide with a real capability / event-queue name.
pub(crate) const CAPS_FAILURE_PREFIX: &str = "\0caps-failure\0";

/// Reports that a CAPS request for `cap` failed before producing a reply,
/// sending the failure sentinel over `caps_tx`. Helpers call this in place of
/// silently swallowing a transport / parse error; the run loop turns it into a
/// diagnostic.
pub(crate) async fn report_caps_failure(caps_tx: &mpsc::Sender<(String, Llsd)>, cap: &str) {
    caps_tx
        .send((format!("{CAPS_FAILURE_PREFIX}{cap}"), Llsd::Undef))
        .await
        .ok();
}

/// Aborts a running task handle, if present.
pub(crate) fn abort_task(task: &mut Option<tokio::task::JoinHandle<()>>) {
    if let Some(handle) = task.take() {
        handle.abort();
    }
}

/// Fetches the region's capability map by POSTing the seed with the requested
/// capability names, returning the cap-name → URL map (empty on any failure or
/// if no seed is known yet).
pub(crate) async fn fetch_capabilities(
    seed: Option<&url::Url>,
    http: &ReqwestClient,
) -> Result<HashMap<String, String>, crate::Error> {
    let seed_url = seed.ok_or_else(|| crate::Error::NoCapabilities {
        message: "the login response carried no capability-seed URL".to_owned(),
    })?;
    let response = http
        .post(seed_url.clone())
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
        .await?;
    let text = response.text().await?;
    parse_seed_response(&text).map_err(|error| crate::Error::NoCapabilities {
        message: format!("the seed-capabilities response did not parse: {error}"),
    })
}

/// GETs the `SimulatorFeatures` capability (when the region advertises it),
/// forwarding the region's feature flags to `caps_tx` for decoding into
/// [`Event::SimulatorFeatures`](sl_proto::Event::SimulatorFeatures). The viewer
/// fetches this automatically on arriving in a region, so the runtime fires it
/// once the capability map is known (at login and on each region change), with
/// no command needed.
pub(crate) fn spawn_simulator_features(
    caps: &HashMap<String, String>,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) {
    if let Some(url) = caps.get(CAP_SIMULATOR_FEATURES).cloned() {
        tokio::spawn(crate::http::get_caps_llsd(
            url,
            CAP_SIMULATOR_FEATURES,
            http.clone(),
            caps_tx.clone(),
        ));
    }
}

/// Spawns the event-queue long-poll task for the `EventQueueGet` capability in
/// `caps`, or `None` if the region did not provide one.
pub(crate) fn spawn_event_queue(
    caps: &HashMap<String, String>,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) -> Option<tokio::task::JoinHandle<()>> {
    let event_queue_url = caps.get("EventQueueGet")?.clone();
    Some(tokio::spawn(run_event_queue(
        event_queue_url,
        http.clone(),
        caps_tx.clone(),
    )))
}

/// Long-polls the `EventQueueGet` capability at `event_queue_url`, forwarding each
/// decoded event to `caps_tx` until a request fails fatally or the receiver is
/// dropped (e.g. on region change).
pub(crate) async fn run_event_queue(
    event_queue_url: String,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let mut ack: Option<i32> = None;
    loop {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_error) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).await.is_err() {
                return;
            }
        }
    }
}

/// Builds a sleep future firing at `deadline`, or far in the future when there
/// is no scheduled timeout.
pub(crate) fn make_sleep(deadline: Option<Instant>) -> tokio::time::Sleep {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)),
        None => tokio::time::sleep(IDLE_SLEEP),
    }
}
