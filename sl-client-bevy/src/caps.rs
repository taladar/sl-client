//! CAPS subsystem lifecycle: seed/map fetch and the EventQueueGet long-poll.
//!
//! The event queue is served by a **single long-lived worker thread** that polls
//! exactly one region's `EventQueueGet` at a time (the current root) and
//! re-targets on a [`EqCommand::Switch`] when the agent changes region. Using one
//! thread — rather than dropping and respawning a poller on every `RegionChanged`
//! — is what makes concurrent duplicate pollers against the *same* queue
//! structurally impossible: under rapid teleports/crossings the old respawn model
//! left a poller lingering (up to `EVENT_QUEUE_TIMEOUT`) on a region's queue while
//! a second poller started on the same queue, corrupting `EventQueueGet`'s
//! ack-sequenced id stream and dropping events (a lost `CrossedRegion` froze the
//! avatar). The events channel is persistent across region changes, so no batch
//! is discarded mid-transition; a switch abandons the old region's queue (the sim
//! retires it on timeout) rather than trying to close it, because a `done: true`
//! close blocks like a long-poll on OpenSim and would stall the switch.

use crate::retry::{MAX_TRANSIENT_RETRIES, transient_backoff};
use crate::{Caps, EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, unbounded};
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_proto::{
    Llsd, REQUESTED_CAPABILITIES, Session, build_event_queue_request, build_seed_request,
    parse_event_queue_response, parse_seed_response,
};
use std::collections::HashMap;
use std::time::Duration;

/// The reserved `(message, body)` key a CAPS helper sends over the events
/// channel when its request failed before producing a reply. The driver
/// recognises the `\0caps-failure\0` prefix, logs it, and — when diagnostics are
/// enabled — surfaces a [`Diagnostic::ExpectedReplyMissing`](sl_proto::Diagnostic::ExpectedReplyMissing)
/// instead of passing it to
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event). The NUL
/// prefix cannot collide with a real capability / event-queue name.
pub(crate) const CAPS_FAILURE_PREFIX: &str = "\0caps-failure\0";

/// How many times a failed seed-capabilities fetch is retried before the worker
/// stops trying and waits for the next region change. Shares the asset fetchers'
/// transient-error budget: the seed POST fails for the same reasons (a sim still
/// spinning up the new region's cap handlers answers a transient error before it
/// answers the map). Without the retry a single transient failure leaves the
/// region with **no event queue at all** until the agent crosses again — and
/// `CrossedRegion` is itself an event-queue event, so the agent cannot.
const MAX_SEED_FETCH_RETRIES: u32 = MAX_TRANSIENT_RETRIES;

/// The pause before re-polling `EventQueueGet` after a round that produced no
/// usable reply — a transport error, an unreadable body, or a body that did not
/// parse. Without it an endpoint that answers `200` with a broken body turns the
/// long poll into an unthrottled request loop.
const POLL_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// Reports that a CAPS request for `cap` failed before producing a reply,
/// sending the failure sentinel over `caps_tx`. Helpers call this in place of
/// silently swallowing a transport / parse error; the driver turns it into a
/// diagnostic.
pub(crate) fn report_caps_failure(caps_tx: &Sender<(String, Llsd)>, cap: &str) {
    deliver(
        caps_tx,
        (format!("{CAPS_FAILURE_PREFIX}{cap}"), Llsd::Undef),
    );
}

/// A command to the single event-queue worker thread.
pub(crate) enum EqCommand {
    /// Re-target the poll to this region's seed capability (a region change). The
    /// worker abandons the old region's queue and begins polling this one.
    Switch(url::Url),
}

/// Starts the CAPS subsystem for the session's current seed capability: a single
/// background worker thread that fetches the capability map (reported over
/// `map_rx`) and long-polls `EventQueueGet`, re-targeting on
/// [`Caps::switch_to`]. Returns `None` if no seed is known yet.
pub(crate) fn start_caps(session: &Session) -> Option<Caps> {
    let Some(seed) = session.seed_capability().map(url::Url::to_owned) else {
        tracing::warn!("start_caps: no seed capability yet — event queue NOT started");
        return None;
    };
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let (command_tx, command_rx) = unbounded();
    let thread_events = events_tx.clone();
    let initial = seed.clone();
    tracing::info!(%seed, "start_caps: event-queue worker starting for the root region");
    std::thread::spawn(move || run_event_queue(initial, &command_rx, &thread_events, &map_tx));
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        map_rx,
        map: HashMap::new(),
        command_tx,
    })
}

impl Caps {
    /// Re-targets the event-queue worker at the session's current root seed,
    /// reusing the single long-lived thread instead of spawning a new poller.
    /// Called on every `RegionChanged`. Always sends the command: the worker
    /// no-ops a switch to the seed it is already polling, so a redundant region
    /// change is cheap, and a re-send re-establishes the queue if a prior fetch
    /// had failed.
    pub(crate) fn switch_to(&self, session: &Session) {
        let Some(seed) = session.seed_capability() else {
            return;
        };
        if self
            .command_tx
            .send(EqCommand::Switch(seed.to_owned()))
            .is_err()
        {
            tracing::warn!(%seed, "switch_to: the event-queue worker has exited");
        } else {
            tracing::info!(%seed, "switch_to: event queue re-targeted to the new root region");
        }
    }
}

/// POSTs a neighbour region's seed capability (in a detached thread, result
/// ignored) so the simulator marks the agent's capabilities as sent and begins
/// streaming that region's scene to the child circuit.
pub(crate) fn post_neighbour_seed(seed_url: url::Url) {
    std::thread::spawn(move || {
        let Ok(http) = crate::http_proxy::blocking_client_builder()
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

/// The outcome of one seed-capabilities fetch, distinguishing the two ways it
/// can leave the worker without a queue to poll: a *failed* fetch is worth
/// retrying, a region that simply advertises no `EventQueueGet` is not.
enum SeedOutcome {
    /// The region served its capability map. Carries its `EventQueueGet` URL, or
    /// `None` when the region advertises no event queue at all — retrying that
    /// fetch would return the same answer.
    Fetched(Option<String>),
    /// The seed request, its body, or its parse failed. Retryable.
    Failed,
}

/// POSTs `seed_url` to fetch a region's capability map (reporting it over
/// `map_tx`, `Ok` or a readable `Err`) and returns its `EventQueueGet` URL —
/// see [`SeedOutcome`] for the two failure shapes.
fn fetch_caps(
    http: &ReqwestBlockingClient,
    seed_url: &url::Url,
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) -> SeedOutcome {
    let response = match http
        .post(seed_url.clone())
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%seed_url, %error, "event queue: seed-capabilities POST failed — no queue for this region");
            deliver(
                map_tx,
                Err(format!("the seed-capabilities request failed: {error}")),
            );
            return SeedOutcome::Failed;
        }
    };
    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            deliver(
                map_tx,
                Err(format!(
                    "the seed-capabilities response body could not be read: {error}"
                )),
            );
            return SeedOutcome::Failed;
        }
    };
    let capabilities = match parse_seed_response(&text) {
        Ok(capabilities) => capabilities,
        Err(error) => {
            deliver(
                map_tx,
                Err(format!(
                    "the seed-capabilities response did not parse: {error}"
                )),
            );
            return SeedOutcome::Failed;
        }
    };
    deliver(map_tx, Ok(capabilities.clone()));
    let url = capabilities.get("EventQueueGet").cloned();
    if url.is_none() {
        tracing::warn!(
            %seed_url,
            caps = capabilities.len(),
            "event queue: region advertises NO EventQueueGet — no CrossedRegion / EnableSimulator will arrive"
        );
    }
    SeedOutcome::Fetched(url)
}

/// The outcome of one `EventQueueGet` long-poll round.
enum PollOutcome {
    /// The round forwarded one or more events.
    Delivered {
        /// `true` when the batch carried a `CrossedRegion` / `TeleportFinish` — a
        /// root-region change the driver will answer with a [`Caps::switch_to`],
        /// so the worker should wait (bounded) for that switch rather than re-poll
        /// the now-stale queue.
        region_change: bool,
    },
    /// The round returned no events (idle timeout, transient error, or unparsable
    /// body) — re-poll with the same ack.
    Idle,
    /// The events receiver was dropped (the [`Caps`] is gone): the worker must
    /// exit.
    ReceiverGone,
}

/// The single event-queue worker's per-region state: the HTTP client, the seed
/// it is polling, that region's `EventQueueGet` URL, and the last ack id.
struct EventQueueWorker {
    /// The blocking HTTP client used for the seed fetch and the long-poll.
    http: ReqwestBlockingClient,
    /// The seed capability of the region currently being polled.
    seed: url::Url,
    /// The current region's `EventQueueGet` URL, or `None` if it has no event
    /// queue (or none has been fetched yet).
    event_queue_url: Option<String>,
    /// The id of the last delivered batch, acked on the next poll; `None` starts
    /// a fresh poll (all queued events).
    ack: Option<i32>,
    /// How many seed fetches for the current region have failed in a row; `0`
    /// once one succeeds. Drives the retry backoff in [`run_event_queue`], so a
    /// transient failure does not cost the region its event queue outright.
    seed_failures: u32,
}

impl EventQueueWorker {
    /// Builds the worker for `seed`, fetching its capability map + event-queue
    /// URL. `None` if the HTTP client could not be built.
    fn new(
        seed: url::Url,
        map_tx: &Sender<Result<HashMap<String, String>, String>>,
    ) -> Option<Self> {
        let http = match crate::http_proxy::blocking_client_builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        {
            Ok(http) => http,
            Err(error) => {
                deliver(
                    map_tx,
                    Err(format!("could not build the caps HTTP client: {error}")),
                );
                return None;
            }
        };
        let outcome = fetch_caps(&http, &seed, map_tx);
        let mut worker = Self {
            http,
            seed,
            event_queue_url: None,
            ack: None,
            seed_failures: 0,
        };
        worker.apply_seed(outcome);
        if let Some(url) = &worker.event_queue_url {
            tracing::info!(%url, "event queue: polling started");
        }
        Some(worker)
    }

    /// Records a seed fetch's outcome: a served map ends the retry budget
    /// (whatever it advertises), a failed fetch spends one more of it.
    fn apply_seed(&mut self, outcome: SeedOutcome) {
        match outcome {
            SeedOutcome::Fetched(url) => {
                self.event_queue_url = url;
                self.seed_failures = 0;
            }
            SeedOutcome::Failed => {
                self.event_queue_url = None;
                self.seed_failures = self.seed_failures.saturating_add(1);
            }
        }
    }

    /// The pause before the next seed-fetch retry, or `None` when there is
    /// nothing to retry — the last fetch succeeded (so the region either has a
    /// queue or genuinely advertises none), or the budget is spent.
    fn seed_retry_backoff(&self) -> Option<Duration> {
        if self.seed_failures == 0 || self.seed_failures > MAX_SEED_FETCH_RETRIES {
            return None;
        }
        Some(transient_backoff(self.seed_failures.saturating_sub(1)))
    }

    /// Re-runs the seed fetch for the region already being polled, after a
    /// failure. Unlike [`EventQueueWorker::switch`] it keeps the failure count,
    /// so the retry budget runs out instead of retrying for ever.
    fn retry_seed(&mut self, map_tx: &Sender<Result<HashMap<String, String>, String>>) {
        tracing::info!(
            seed = %self.seed,
            failures = self.seed_failures,
            "event queue: retrying the seed-capabilities fetch"
        );
        let outcome = fetch_caps(&self.http, &self.seed, map_tx);
        self.apply_seed(outcome);
        if let Some(url) = &self.event_queue_url {
            tracing::info!(%url, "event queue: polling started after a seed retry");
        }
    }

    /// Re-targets the worker at `seed` (a region change): **abandons** the old
    /// region's queue and fetches the new region's map + event-queue URL. A no-op
    /// if already on `seed` with a live queue.
    ///
    /// We deliberately do **not** send a `done: true` close on the old queue. On
    /// OpenSim that POST blocks like a normal long-poll (measured ~58 s to the
    /// timeout), which would stall the switch and hang the crossing; and doing it
    /// in a detached thread would leave a request racing the worker's own poll if
    /// the agent crosses straight back into the region. The events that mattered
    /// were already delivered before this switch (the `CrossedRegion` that
    /// triggered it), the old region's trailing events are irrelevant once we have
    /// crossed away, and the sim retires the abandoned queue on its own timeout —
    /// a re-visit then polls it fresh, still with a single poller.
    fn switch(&mut self, seed: url::Url, map_tx: &Sender<Result<HashMap<String, String>, String>>) {
        if seed == self.seed && self.event_queue_url.is_some() {
            return;
        }
        if let Some(old_url) = self.event_queue_url.take() {
            tracing::debug!(%old_url, "event queue: abandoning the old region queue on switch");
        }
        self.seed = seed;
        self.ack = None;
        // A new region starts with a fresh retry budget: the old region's
        // failures say nothing about this one's.
        self.seed_failures = 0;
        let outcome = fetch_caps(&self.http, &self.seed, map_tx);
        self.apply_seed(outcome);
        if let Some(url) = &self.event_queue_url {
            tracing::info!(%url, "event queue: re-targeted to the new region");
        }
    }

    /// Performs one `EventQueueGet` long-poll round against the current queue,
    /// forwarding every decoded event to `caps_tx`. A pulled batch is forwarded in
    /// full before returning, so an event taken off the wire is never discarded.
    fn poll_round(&mut self, caps_tx: &Sender<(String, Llsd)>) -> PollOutcome {
        let Some(url) = self.event_queue_url.clone() else {
            return PollOutcome::Idle;
        };
        let response = match self
            .http
            .post(&url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(self.ack, false))
            .send()
        {
            Ok(response) => response,
            Err(error) => {
                tracing::trace!(%error, "event queue: poll request errored; retrying");
                std::thread::sleep(POLL_ERROR_BACKOFF);
                return PollOutcome::Idle;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with the
        // same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            return PollOutcome::Idle;
        }
        // A body that cannot be read, or that does not parse, is as transient as
        // a failed request — and backs off the same way, or a grid answering a
        // broken `200` spins this thread as fast as the network allows.
        let text = match response.text() {
            Ok(text) => text,
            Err(error) => {
                tracing::trace!(%error, "event queue: poll body could not be read; retrying");
                std::thread::sleep(POLL_ERROR_BACKOFF);
                return PollOutcome::Idle;
            }
        };
        let parsed = match parse_event_queue_response(&text) {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::trace!(%error, "event queue: poll body did not parse; retrying");
                std::thread::sleep(POLL_ERROR_BACKOFF);
                return PollOutcome::Idle;
            }
        };
        self.ack = Some(parsed.id);
        if parsed.events.is_empty() {
            return PollOutcome::Idle;
        }
        let names: Vec<&str> = parsed
            .events
            .iter()
            .map(|event| event.message.as_str())
            .collect();
        let region_change = names
            .iter()
            .any(|name| *name == "CrossedRegion" || *name == "TeleportFinish");
        tracing::debug!(
            id = parsed.id,
            count = names.len(),
            ?names,
            "event queue: delivering batch"
        );
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).is_err() {
                return PollOutcome::ReceiverGone;
            }
        }
        PollOutcome::Delivered { region_change }
    }
}

/// The single event-queue worker loop: poll the current region's queue, applying
/// any pending [`EqCommand::Switch`] between rounds. Exits when the command
/// channel closes (the [`Caps`] was dropped) or the events receiver is gone.
fn run_event_queue(
    initial_seed: url::Url,
    command_rx: &Receiver<EqCommand>,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) {
    let Some(mut worker) = EventQueueWorker::new(initial_seed, map_tx) else {
        return;
    };
    loop {
        // Apply every queued switch before polling, so a just-issued region change
        // is honoured before we poll a now-stale queue.
        loop {
            match command_rx.try_recv() {
                Ok(EqCommand::Switch(seed)) => worker.switch(seed, map_tx),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if worker.event_queue_url.is_none() {
            // Nothing to poll. A *failed* seed fetch is retried on a backoff — a
            // region that advertises no event queue answers the same way every
            // time, so only a failure is worth repeating, and leaving a region
            // queueless over one transient error strands the agent (its own
            // `CrossedRegion` is an event-queue event). Once the budget is spent,
            // block for a command so the thread does not spin.
            match worker.seed_retry_backoff() {
                Some(backoff) => match command_rx.recv_timeout(backoff) {
                    Ok(EqCommand::Switch(seed)) => worker.switch(seed, map_tx),
                    Err(RecvTimeoutError::Timeout) => worker.retry_seed(map_tx),
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match command_rx.recv() {
                    Ok(EqCommand::Switch(seed)) => worker.switch(seed, map_tx),
                    Err(_) => return,
                },
            }
            continue;
        }
        match worker.poll_round(caps_tx) {
            PollOutcome::ReceiverGone => return,
            PollOutcome::Delivered {
                region_change: true,
            } => {
                // The batch carried a CrossedRegion / TeleportFinish: the driver
                // will re-target us to the new region. Wait (bounded) for that
                // switch before polling the now-stale queue again — a re-poll here
                // could block a full long-poll on a region we have already left.
                // The bound lets us resume if no switch comes (e.g. a teleport that
                // fails), and is generous enough to cover a slow frame under load.
                match command_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(EqCommand::Switch(seed)) => worker.switch(seed, map_tx),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            PollOutcome::Delivered {
                region_change: false,
            }
            | PollOutcome::Idle => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{EventQueueWorker, MAX_SEED_FETCH_RETRIES, SeedOutcome};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// A worker parked on a placeholder seed, with no fetch performed: the retry
    /// budget is driven directly through [`EventQueueWorker::apply_seed`], so the
    /// state machine is exercised without a grid.
    fn worker() -> EventQueueWorker {
        EventQueueWorker {
            http: crate::http_proxy::blocking_client_builder()
                .build()
                .expect("a direct blocking client builds"),
            seed: "http://sim.example/cap/seed"
                .parse()
                .expect("the placeholder seed URL parses"),
            event_queue_url: None,
            ack: None,
            seed_failures: 0,
        }
    }

    /// A region that served its map has nothing to retry — whether or not it
    /// advertised an event queue. Retrying a region that answered "no
    /// `EventQueueGet`" would only re-read the same answer.
    #[test]
    fn a_served_map_ends_the_retry_budget() {
        let mut worker = worker();
        worker.apply_seed(SeedOutcome::Fetched(Some(
            "http://sim.example/eq".to_owned(),
        )));
        assert_eq!(worker.seed_failures, 0);
        assert_eq!(worker.seed_retry_backoff(), None);

        worker.apply_seed(SeedOutcome::Fetched(None));
        assert_eq!(worker.event_queue_url, None);
        assert_eq!(worker.seed_retry_backoff(), None);
    }

    /// Each consecutive failure spends one retry and lengthens the pause, until
    /// the budget runs out and the worker falls back to waiting for the next
    /// region change.
    #[test]
    fn consecutive_failures_back_off_then_exhaust_the_budget() {
        let mut worker = worker();
        worker.apply_seed(SeedOutcome::Failed);
        assert_eq!(
            worker.seed_retry_backoff(),
            Some(Duration::from_millis(200))
        );
        worker.apply_seed(SeedOutcome::Failed);
        assert_eq!(
            worker.seed_retry_backoff(),
            Some(Duration::from_millis(400))
        );

        for _spent in 0..MAX_SEED_FETCH_RETRIES {
            worker.apply_seed(SeedOutcome::Failed);
        }
        assert!(worker.seed_failures > MAX_SEED_FETCH_RETRIES);
        assert_eq!(worker.seed_retry_backoff(), None);
    }

    /// A success in the middle of a failing run re-opens the full budget, so a
    /// region that flaps is not starved by an earlier region's failures.
    #[test]
    fn a_success_restores_the_full_budget() {
        let mut worker = worker();
        for _spent in 0..MAX_SEED_FETCH_RETRIES {
            worker.apply_seed(SeedOutcome::Failed);
        }
        worker.apply_seed(SeedOutcome::Fetched(Some(
            "http://sim.example/eq".to_owned(),
        )));
        worker.apply_seed(SeedOutcome::Failed);
        assert_eq!(
            worker.seed_retry_backoff(),
            Some(Duration::from_millis(200))
        );
    }
}
