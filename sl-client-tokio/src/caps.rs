//! CAPS lifecycle: capability fetch, event-queue spawn/poll, task helpers.

use crate::IDLE_SLEEP;
use crate::retry::{MAX_TRANSIENT_RETRIES, transient_backoff};
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

/// The synthetic tag a failed region-change seed-capabilities fetch is reported
/// under, following the [`CAPS_FAILURE_PREFIX`] convention. Never a real
/// capability name and never routed to
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event) — the
/// run loop strips the prefix and turns it into a diagnostic, so a region whose
/// capability map could not be fetched is *visible* rather than silently
/// stripped of its event queue, asset caps and inventory.
pub(crate) const SEED_CAPABILITIES_TAG: &str = "Seed/capabilities";

/// How many times a region-change seed-capabilities fetch is retried before
/// [`refetch_capabilities`] gives up and reports the failure. Shares the asset
/// fetchers' transient-error budget: the seed POST fails for the same reasons
/// (a sim still spinning up the new region's cap handlers answers a transient
/// error before it answers the map).
const MAX_SEED_FETCH_RETRIES: u32 = MAX_TRANSIENT_RETRIES;

/// The pause before re-polling `EventQueueGet` after a round that produced no
/// usable reply — a transport error, an unreadable body, or a body that did not
/// parse. Without it an endpoint that answers `200` with a broken body turns the
/// long poll into an unthrottled request loop.
const POLL_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// Reports that a CAPS request for `cap` failed before producing a reply,
/// sending the failure sentinel over `caps_tx`. Helpers call this in place of
/// silently swallowing a transport / parse error; the run loop turns it into a
/// diagnostic.
pub(crate) async fn report_caps_failure(caps_tx: &mpsc::Sender<(String, Llsd)>, cap: &str) {
    deliver(
        caps_tx,
        (format!("{CAPS_FAILURE_PREFIX}{cap}"), Llsd::Undef),
    )
    .await;
}

/// Hand a worker task's result back to the run loop over one of the session's
/// channels (events, diagnostics, CAPS payloads, the caps reporter).
///
/// The **only** way this fails is a closed channel, which means the receiver was
/// dropped: the `Client` went away, the consumer stopped reading, or the run
/// loop already ended. The result belongs to a session nobody is driving any
/// more, so there is nothing to report it to — routing every such send through
/// this one helper keeps it the only place a send result is discarded.
pub(crate) async fn deliver<T>(tx: &mpsc::Sender<T>, value: T) {
    tx.send(value).await.ok();
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

/// Re-fetches a region's capability map **off the run loop** after a region
/// change, retrying a transient failure with exponential backoff, and hands the
/// result to the run loop over `map_tx` stamped with `generation`.
///
/// Region change is the one capability fetch that must not run inline. The run
/// loop is the session's only UDP pump, so awaiting a seed round-trip there
/// stalls `recv_from`, the ACKs and the retransmits for as long as the grid
/// takes to answer — up to the client's 60 s HTTP timeout, long enough for the
/// simulator to drop the circuit. (The initial-login fetch at the head of
/// [`Client::run`](crate::Client::run) is deliberately inline instead: nothing
/// is pumping yet, and a region that serves no capabilities must fail the login
/// rather than be entered.)
///
/// A failure that survives every retry is reported over `caps_tx` under
/// [`SEED_CAPABILITIES_TAG`] rather than swallowed into an empty map, and the
/// run loop keeps the previous region's map until a fetch succeeds — a
/// half-degraded region beats one whose every capability silently vanished.
pub(crate) async fn refetch_capabilities(
    generation: u64,
    seed: Option<url::Url>,
    http: ReqwestClient,
    map_tx: mpsc::Sender<(u64, HashMap<String, String>)>,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let Some(seed) = seed else {
        tracing::warn!("the region change carried no capability-seed URL");
        report_caps_failure(&caps_tx, SEED_CAPABILITIES_TAG).await;
        return;
    };
    for attempt in 0..=MAX_SEED_FETCH_RETRIES {
        match fetch_capabilities(Some(&seed), &http).await {
            Ok(capabilities) => {
                deliver(&map_tx, (generation, capabilities)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(
                    %seed,
                    %error,
                    attempt,
                    "the region-change seed-capabilities fetch failed"
                );
                if attempt < MAX_SEED_FETCH_RETRIES {
                    tokio::time::sleep(transient_backoff(attempt)).await;
                }
            }
        }
    }
    report_caps_failure(&caps_tx, SEED_CAPABILITIES_TAG).await;
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
                tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        // A body that cannot be read, or that does not parse, is as transient as
        // a failed request — and back off the same way, or a grid answering a
        // broken `200` spins this task as fast as the network allows.
        let text = match response.text().await {
            Ok(text) => text,
            Err(_error) => {
                tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                continue;
            }
        };
        let parsed = match parse_event_queue_response(&text) {
            Ok(parsed) => parsed,
            Err(_error) => {
                tokio::time::sleep(POLL_ERROR_BACKOFF).await;
                continue;
            }
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{CAPS_FAILURE_PREFIX, SEED_CAPABILITIES_TAG, refetch_capabilities};
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    /// The `(message, body)` key a reported seed failure arrives under.
    fn failure_key() -> String {
        format!("{CAPS_FAILURE_PREFIX}{SEED_CAPABILITIES_TAG}")
    }

    /// A region change that carried no seed capability cannot be fetched at all:
    /// it reports the failure immediately — no retries, and no empty map passed
    /// off as the region's capabilities.
    #[tokio::test]
    async fn a_region_change_without_a_seed_reports_the_failure() {
        let http = crate::http_proxy::client_builder()
            .build()
            .expect("a direct client builds");
        let (map_tx, mut map_rx) = mpsc::channel::<(u64, HashMap<String, String>)>(4);
        let (caps_tx, mut caps_rx) = mpsc::channel(4);

        refetch_capabilities(1, None, http, map_tx, caps_tx).await;

        assert!(
            map_rx.try_recv().is_err(),
            "a failed fetch must not install a capability map"
        );
        let (message, _body) = caps_rx.try_recv().expect("the failure is reported");
        assert_eq!(message, failure_key());
        assert!(
            caps_rx.try_recv().is_err(),
            "exactly one failure is reported"
        );
    }

    /// A seed URL nothing is listening on fails every attempt: the refetch spends
    /// its whole retry budget, then reports exactly one failure and still
    /// installs no map. Time is paused so the backoffs cost no wall clock.
    #[tokio::test(start_paused = true)]
    async fn a_dead_seed_exhausts_the_budget_then_reports_once() {
        // Bind and drop, so the port is known-free and the connect is refused
        // rather than left hanging on the timeout.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port binds");
        let address = listener.local_addr().expect("the bound port is readable");
        drop(listener);
        let seed: url::Url = format!("http://{address}/cap/seed")
            .parse()
            .expect("the seed URL parses");

        let http = crate::http_proxy::client_builder()
            .build()
            .expect("a direct client builds");
        let (map_tx, mut map_rx) = mpsc::channel::<(u64, HashMap<String, String>)>(4);
        let (caps_tx, mut caps_rx) = mpsc::channel(4);

        refetch_capabilities(2, Some(seed), http, map_tx, caps_tx).await;

        assert!(
            map_rx.try_recv().is_err(),
            "a failed fetch must not install a capability map"
        );
        let (message, _body) = caps_rx.try_recv().expect("the failure is reported");
        assert_eq!(message, failure_key());
        assert!(
            caps_rx.try_recv().is_err(),
            "exactly one failure is reported, not one per retry"
        );
    }
}
