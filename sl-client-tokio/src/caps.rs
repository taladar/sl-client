//! CAPS lifecycle: capability fetch, event-queue spawn/poll, task helpers.

use crate::IDLE_SLEEP;
use reqwest::Client as ReqwestClient;
use sl_proto::{
    Llsd, REQUESTED_CAPABILITIES, build_event_queue_request, build_seed_request,
    parse_event_queue_response, parse_seed_response,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

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
    seed: Option<&str>,
    http: &ReqwestClient,
) -> HashMap<String, String> {
    let Some(seed_url) = seed else {
        return HashMap::new();
    };
    let result = http
        .post(seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
        .await;
    let Ok(response) = result else {
        return HashMap::new();
    };
    let Ok(text) = response.text().await else {
        return HashMap::new();
    };
    parse_seed_response(&text).unwrap_or_default()
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
