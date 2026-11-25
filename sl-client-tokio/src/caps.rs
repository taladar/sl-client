//! CAPS lifecycle: capability fetch, the seed POSTs and long polls the
//! [`EventQueues`](sl_client_common::event_queues::EventQueues) plan asks for,
//! task helpers.

use crate::IDLE_SLEEP;
use reqwest::Client as ReqwestClient;
use sl_client_common::event_queues::CapabilityMap;
use sl_proto::{
    CAP_SIMULATOR_FEATURES, Llsd, build_event_queue_request, build_seed_request,
    parse_event_queue_response, parse_seed_response,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
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

/// Fetches the region's capability map by POSTing the seed with the requested
/// capability names, returning the cap-name → URL map (empty on any failure or
/// if no seed is known yet).
pub(crate) async fn fetch_capabilities(
    seed: Option<&url::Url>,
    http: &ReqwestClient,
    requested: &[String],
) -> Result<HashMap<String, String>, crate::Error> {
    let seed_url = seed.ok_or_else(|| crate::Error::NoCapabilities {
        message: "the login response carried no capability-seed URL".to_owned(),
    })?;
    let response = http
        .post(seed_url.clone())
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(
            &requested.iter().map(String::as_str).collect::<Vec<_>>(),
        ))
        .send()
        .await?;
    // Kept for the message: a region that answers `404` with no body would
    // otherwise read as a document that failed to parse.
    let status = response.status();
    let text = response.text().await?;
    parse_seed_response(&text).map_err(|error| crate::Error::NoCapabilities {
        message: format!("the seed-capabilities response (HTTP {status}) did not parse: {error}"),
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

/// A seed POST that has returned.
pub(crate) struct FinishedSeed {
    /// The region it was for.
    pub(crate) sim: SocketAddr,
    /// The seed it POSTed.
    pub(crate) seed: url::Url,
    /// The map, or why there is none.
    pub(crate) outcome: Result<CapabilityMap, String>,
}

/// A decoded `EventQueueGet` reply: its batch id and its `(message, body)`
/// events.
pub(crate) type PollReply = (i32, Vec<(String, Llsd)>);

/// A long poll that has returned.
pub(crate) struct FinishedPoll {
    /// The region whose queue it polled.
    pub(crate) sim: SocketAddr,
    /// The queue.
    pub(crate) url: String,
    /// Its reply, or `None` for an idle timeout or a transient error.
    pub(crate) reply: Option<PollReply>,
}

/// POSTs `seed` for the region at `sim` (one attempt; the plan schedules the
/// retries) and reports what it answered over `done_tx`.
pub(crate) async fn run_seed_fetch(
    sim: SocketAddr,
    seed: url::Url,
    requested: Arc<[String]>,
    http: ReqwestClient,
    done_tx: mpsc::Sender<FinishedSeed>,
) {
    let outcome = fetch_capabilities(Some(&seed), &http, &requested)
        .await
        .map_err(|error| error.to_string());
    deliver(&done_tx, FinishedSeed { sim, seed, outcome }).await;
}

/// Runs one `EventQueueGet` long poll on `url`, the queue of the region at
/// `sim`, and reports its reply over `done_tx`. A failed round has already
/// waited out its backoff when it is reported, so the queue stays marked as
/// polled until it may be polled again.
pub(crate) async fn run_poll(
    sim: SocketAddr,
    url: String,
    ack: Option<i32>,
    http: ReqwestClient,
    done_tx: mpsc::Sender<FinishedPoll>,
) {
    let reply = poll_once(&url, ack, &http).await;
    deliver(&done_tx, FinishedPoll { sim, url, reply }).await;
}

/// One `EventQueueGet` long poll on `event_queue_url`: its batch, or `None`
/// for a round that carried none, after the round's backoff.
async fn poll_once(
    event_queue_url: &str,
    ack: Option<i32>,
    http: &ReqwestClient,
) -> Option<PollReply> {
    let response = match http
        .post(event_queue_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_event_queue_request(ack, false))
        .send()
        .await
    {
        Ok(response) => response,
        // A long poll the grid holds past the client's timeout ends here
        // every idle minute, so this is not worth more than a debug line.
        Err(error) => {
            tracing::debug!(%event_queue_url, "event queue: the poll failed: {error}");
            tokio::time::sleep(POLL_ERROR_BACKOFF).await;
            return None;
        }
    };
    // A timeout with no events returns a non-2xx (e.g. 502); re-poll with the
    // same ack after a short pause.
    if !response.status().is_success() {
        tokio::time::sleep(Duration::from_millis(200)).await;
        return None;
    }
    // A body that cannot be read, or that does not parse, is as transient as a
    // failed request — and backs off the same way, or a grid answering a
    // broken `200` spins as fast as the network allows.
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%event_queue_url, "event queue: the reply body could not be read: {error}");
            tokio::time::sleep(POLL_ERROR_BACKOFF).await;
            return None;
        }
    };
    let parsed = match parse_event_queue_response(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(%event_queue_url, "event queue: the reply did not parse: {error}");
            tokio::time::sleep(POLL_ERROR_BACKOFF).await;
            return None;
        }
    };
    let events = parsed
        .events
        .into_iter()
        .map(|event| (event.message, event.body))
        .collect();
    Some((parsed.id, events))
}

/// The earlier of two optional deadlines.
pub(crate) fn earliest(first: Option<Instant>, second: Option<Instant>) -> Option<Instant> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (first, None) => first,
        (None, second) => second,
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

    use super::{FinishedSeed, run_seed_fetch};
    use pretty_assertions::assert_eq;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// A seed URL nothing is listening on is reported as a failure, keyed by
    /// its region and seed, so the plan can schedule the retry.
    #[tokio::test]
    async fn a_dead_seed_reports_its_failure() {
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
        let (done_tx, mut done_rx) = mpsc::channel(1);
        let sim = SocketAddr::from(([127, 0, 0, 1], 13_000));

        run_seed_fetch(sim, seed.clone(), Arc::from(Vec::new()), http, done_tx).await;

        let FinishedSeed {
            sim: reported_sim,
            seed: reported_seed,
            outcome,
        } = done_rx.try_recv().expect("the outcome is reported");
        assert_eq!(reported_sim, sim);
        assert_eq!(reported_seed, seed);
        assert!(outcome.is_err(), "a refused connection is a failure");
    }
}
