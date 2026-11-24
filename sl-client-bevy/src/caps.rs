//! CAPS subsystem lifecycle: seed/map fetch and the EventQueueGet long-poll.
//!
//! The event queue is served by a **single long-lived worker thread** that polls
//! the current root region's `EventQueueGet` and re-targets on an
//! [`EqCommand::Switch`] when the agent changes region. Each long poll runs on a
//! thread of its own, so a switch takes effect while the old region's poll is
//! still held open by the simulator.
//!
//! The worker tracks every poll still open and never starts a second one on the
//! same queue, even after leaving a region and coming back. Concurrent pollers
//! on one queue corrupt `EventQueueGet`'s ack-sequenced id stream and drop events
//! (a lost `CrossedRegion` froze the avatar). The events channel is persistent
//! across region changes, so no batch is discarded mid-transition; a switch
//! abandons the old region's queue (the sim retires it on timeout) rather than
//! trying to close it, because a `done: true` close blocks like a long-poll on
//! OpenSim and would stall the switch.

use crate::{Caps, EVENT_QUEUE_TIMEOUT, deliver};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, TryRecvError, after, never, select, unbounded};
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_client_common::retry::{MAX_TRANSIENT_RETRIES, transient_backoff};
use sl_proto::{
    Llsd, Session, build_event_queue_request, build_seed_request, parse_event_queue_response,
    parse_seed_response,
};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
pub(crate) fn start_caps(session: &Session, requested: &Arc<[String]>) -> Option<Caps> {
    let Some(seed) = session.seed_capability().map(url::Url::to_owned) else {
        tracing::warn!("start_caps: no seed capability yet — event queue NOT started");
        return None;
    };
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (chat_replies_tx, chat_replies_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let (command_tx, command_rx) = unbounded();
    let (neighbour_map_tx, neighbour_map_rx) = unbounded();
    let thread_events = events_tx.clone();
    let initial = seed.clone();
    let worker_requested = Arc::clone(requested);
    tracing::info!(%seed, "start_caps: event-queue worker starting for the root region");
    crate::log_context::spawn_thread(move || {
        run_event_queue(
            initial,
            worker_requested,
            &command_rx,
            &thread_events,
            &map_tx,
        );
    });
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        chat_replies_rx,
        chat_replies_tx,
        map_rx,
        map: HashMap::new(),
        neighbour_map_rx,
        neighbour_map_tx,
        command_tx,
        requested: Arc::clone(requested),
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

/// A neighbouring region's capability map, or why it could not be fetched,
/// keyed by the neighbour's simulator address.
pub(crate) type NeighbourMapOutcome = (SocketAddr, Result<HashMap<String, String>, String>);

/// POSTs a neighbour region's seed capability on a detached thread, so the
/// simulator marks the agent's capabilities as sent and begins streaming that
/// region's scene to the child circuit, and reports the neighbour's capability
/// map (or why it could not be had) over `map_tx`, keyed by the neighbour's
/// simulator address — the map object-addressed capability requests about the
/// neighbour's objects are sent to. A failed fetch is retried like the root
/// region's.
pub(crate) fn fetch_neighbour_caps(
    sim: SocketAddr,
    seed_url: url::Url,
    requested: Arc<[String]>,
    map_tx: Sender<NeighbourMapOutcome>,
) {
    crate::log_context::spawn_thread(move || {
        let outcome = match crate::http_proxy::blocking_client_builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        {
            Ok(http) => {
                let mut outcome = post_seed(&http, &seed_url, &requested);
                for attempt in 0..MAX_SEED_FETCH_RETRIES {
                    let Err(reason) = &outcome else {
                        break;
                    };
                    tracing::warn!(%sim, %seed_url, attempt, "neighbour seed-capabilities fetch failed: {reason}");
                    std::thread::sleep(transient_backoff(attempt));
                    outcome = post_seed(&http, &seed_url, &requested);
                }
                outcome
            }
            Err(error) => Err(format!("could not build the caps HTTP client: {error}")),
        };
        deliver(&map_tx, (sim, outcome));
    });
}

/// POSTs `seed_url` and parses the capability map it answers with, or says
/// readably which step failed.
fn post_seed(
    http: &ReqwestBlockingClient,
    seed_url: &url::Url,
    requested: &[String],
) -> Result<HashMap<String, String>, String> {
    let names: Vec<&str> = requested.iter().map(String::as_str).collect();
    let response = http
        .post(seed_url.clone())
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(&names))
        .send()
        .map_err(|error| format!("the seed-capabilities request failed: {error}"))?;
    // Kept for the message: a region that answers `404` with no body would
    // otherwise read as a document that failed to parse.
    let status = response.status();
    let text = response.text().map_err(|error| {
        format!("the seed-capabilities response body could not be read: {error}")
    })?;
    parse_seed_response(&text).map_err(|error| {
        format!("the seed-capabilities response (HTTP {status}) did not parse: {error}")
    })
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
    requested: &[String],
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) -> SeedOutcome {
    let capabilities = match post_seed(http, seed_url, requested) {
        Ok(capabilities) => capabilities,
        Err(reason) => {
            tracing::warn!(%seed_url, "event queue: {reason} — no queue for this region");
            deliver(map_tx, Err(reason));
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

/// A decoded `EventQueueGet` reply: its batch id and its `(message, body)`
/// events.
type PollReply = (i32, Vec<(String, Llsd)>);

/// A long poll that has returned, as its thread reports it to the worker.
struct FinishedPoll {
    /// The queue it polled.
    url: String,
    /// The worker's [`generation`](EventQueueWorker::generation) when it
    /// started. An older one means the worker has switched away since.
    generation: u64,
    /// Its reply, or `None` for an idle timeout or a transient error.
    reply: Option<PollReply>,
}

/// Performs one `EventQueueGet` long poll. A failed round has already waited out
/// its backoff when this returns, so the queue stays marked as polled until it
/// may be polled again.
fn fetch_poll(http: &ReqwestBlockingClient, url: &str, ack: Option<i32>) -> Option<PollReply> {
    let response = match http
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_event_queue_request(ack, false))
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            tracing::trace!(%error, "event queue: poll request errored; retrying");
            std::thread::sleep(POLL_ERROR_BACKOFF);
            return None;
        }
    };
    // A timeout with no events returns a non-2xx (e.g. 502); re-poll with the
    // same ack after a short pause.
    if !response.status().is_success() {
        std::thread::sleep(Duration::from_millis(200));
        return None;
    }
    // A body that cannot be read, or that does not parse, is as transient as
    // a failed request — and backs off the same way, or a grid answering a
    // broken `200` spins this thread as fast as the network allows.
    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            tracing::trace!(%error, "event queue: poll body could not be read; retrying");
            std::thread::sleep(POLL_ERROR_BACKOFF);
            return None;
        }
    };
    let parsed = match parse_event_queue_response(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::trace!(%error, "event queue: poll body did not parse; retrying");
            std::thread::sleep(POLL_ERROR_BACKOFF);
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
    /// The capability names every seed fetch asks for.
    requested: Arc<[String]>,
    /// Every queue with a long poll still open, including queues the worker
    /// has switched away from. A queue listed here is not polled again.
    in_flight: HashSet<String>,
    /// Counts the switches between regions, so a returning poll can tell
    /// whether it belongs to the current visit.
    generation: u64,
}

impl EventQueueWorker {
    /// Builds the worker for `seed`, fetching its capability map + event-queue
    /// URL. `None` if the HTTP client could not be built.
    fn new(
        seed: url::Url,
        requested: Arc<[String]>,
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
        Some(Self::with_client(http, seed, requested, map_tx))
    }

    /// Builds the worker for `seed` on the HTTP client `http`.
    fn with_client(
        http: ReqwestBlockingClient,
        seed: url::Url,
        requested: Arc<[String]>,
        map_tx: &Sender<Result<HashMap<String, String>, String>>,
    ) -> Self {
        let outcome = fetch_caps(&http, &seed, &requested, map_tx);
        let mut worker = Self {
            http,
            seed,
            event_queue_url: None,
            ack: None,
            seed_failures: 0,
            requested,
            in_flight: HashSet::new(),
            generation: 0,
        };
        worker.apply_seed(outcome);
        if let Some(url) = &worker.event_queue_url {
            tracing::info!(%url, "event queue: polling started");
        }
        worker
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
        let outcome = fetch_caps(&self.http, &self.seed, &self.requested, map_tx);
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
    /// the agent crosses straight back into the region. A poll still open on the
    /// old queue runs out on its own thread; a re-visit waits for it
    /// ([`in_flight`](Self::in_flight)) and then polls fresh, still with a single
    /// poller.
    fn switch(&mut self, seed: url::Url, map_tx: &Sender<Result<HashMap<String, String>, String>>) {
        if seed == self.seed && self.event_queue_url.is_some() {
            return;
        }
        if let Some(old_url) = self.event_queue_url.take() {
            tracing::debug!(%old_url, "event queue: abandoning the old region queue on switch");
        }
        self.generation = self.generation.wrapping_add(1);
        self.seed = seed;
        self.ack = None;
        // A new region starts with a fresh retry budget: the old region's
        // failures say nothing about this one's.
        self.seed_failures = 0;
        let outcome = fetch_caps(&self.http, &self.seed, &self.requested, map_tx);
        self.apply_seed(outcome);
        if let Some(url) = &self.event_queue_url {
            tracing::info!(%url, "event queue: re-targeted to the new region");
        }
    }

    /// Starts a long poll on the current queue, unless it has none or a poll
    /// is still open on it from this visit or an earlier one.
    fn start_poll(&mut self, done_tx: &Sender<FinishedPoll>) {
        let Some(url) = self.event_queue_url.clone() else {
            return;
        };
        if !self.in_flight.insert(url.clone()) {
            return;
        }
        let http = self.http.clone();
        let ack = self.ack;
        let generation = self.generation;
        let done_tx = done_tx.clone();
        crate::log_context::spawn_thread(move || {
            let reply = fetch_poll(&http, &url, ack);
            // A closed channel means the worker has exited.
            let _ignored = done_tx.send(FinishedPoll {
                url,
                generation,
                reply,
            });
        });
    }

    /// Takes in a returned poll, forwarding every event it carries, so an event
    /// taken off the wire is never discarded. Only a poll of the current visit
    /// moves the ack or asks the worker to wait for a region switch.
    fn finish_poll(
        &mut self,
        finished: FinishedPoll,
        caps_tx: &Sender<(String, Llsd)>,
    ) -> PollOutcome {
        let FinishedPoll {
            url,
            generation,
            reply,
        } = finished;
        self.in_flight.remove(&url);
        let current =
            generation == self.generation && self.event_queue_url.as_deref() == Some(url.as_str());
        let Some((id, events)) = reply else {
            return PollOutcome::Idle;
        };
        if current {
            self.ack = Some(id);
        }
        if events.is_empty() {
            return PollOutcome::Idle;
        }
        let names: Vec<&str> = events.iter().map(|(name, _body)| name.as_str()).collect();
        let region_change = current
            && names
                .iter()
                .any(|name| *name == "CrossedRegion" || *name == "TeleportFinish");
        tracing::debug!(
            id,
            count = names.len(),
            ?names,
            current,
            "event queue: delivering batch"
        );
        for event in events {
            if caps_tx.send(event).is_err() {
                return PollOutcome::ReceiverGone;
            }
        }
        PollOutcome::Delivered { region_change }
    }
}

/// How long the worker holds off re-polling a queue whose last batch announced a
/// region change, waiting for the driver's switch. The bound lets it resume if no
/// switch comes (a teleport that fails), and covers a slow frame under load.
const REGION_CHANGE_HOLD: Duration = Duration::from_secs(2);

/// Builds the event-queue worker and runs it. Exits when the command channel
/// closes (the [`Caps`] was dropped) or the events receiver is gone.
fn run_event_queue(
    initial_seed: url::Url,
    requested: Arc<[String]>,
    command_rx: &Receiver<EqCommand>,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) {
    if let Some(worker) = EventQueueWorker::new(initial_seed, requested, map_tx) {
        drive_event_queue(worker, command_rx, caps_tx, map_tx);
    }
}

/// The event-queue worker loop: keeps a poll open on the current region's queue
/// and reacts to whichever comes first, a returned poll or an
/// [`EqCommand::Switch`].
fn drive_event_queue(
    mut worker: EventQueueWorker,
    command_rx: &Receiver<EqCommand>,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) {
    let (done_tx, done_rx) = unbounded();
    // Set after a batch announced a region change, so the stale queue is not
    // polled while the driver's switch is on its way.
    let mut hold_until: Option<Instant> = None;
    loop {
        // Apply every queued switch before polling, so a just-issued region change
        // is honoured before we poll a now-stale queue.
        loop {
            match command_rx.try_recv() {
                Ok(EqCommand::Switch(seed)) => {
                    worker.switch(seed, map_tx);
                    hold_until = None;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        hold_until = hold_until.filter(|until| now < *until);
        if hold_until.is_none() {
            worker.start_poll(&done_tx);
        }
        // With no queue, a *failed* seed fetch is retried on a backoff — a region
        // that advertises no event queue answers the same way every time, so only
        // a failure is worth repeating, and leaving a region queueless over one
        // transient error strands the agent (its own `CrossedRegion` is an
        // event-queue event). Once the budget is spent, only a command wakes this.
        let seed_retry = if worker.event_queue_url.is_none() {
            worker.seed_retry_backoff()
        } else {
            None
        };
        let wake = hold_until
            .map(|until| until.saturating_duration_since(now))
            .or(seed_retry);
        let timer = wake.map_or_else(never, after);
        select! {
            recv(command_rx) -> command => match command {
                Ok(EqCommand::Switch(seed)) => {
                    worker.switch(seed, map_tx);
                    hold_until = None;
                }
                Err(_) => return,
            },
            recv(done_rx) -> finished => {
                let Ok(finished) = finished else {
                    return;
                };
                match worker.finish_poll(finished, caps_tx) {
                    PollOutcome::ReceiverGone => return,
                    PollOutcome::Delivered { region_change: true } => {
                        hold_until = Instant::now().checked_add(REGION_CHANGE_HOLD);
                    }
                    PollOutcome::Delivered { region_change: false } | PollOutcome::Idle => {}
                }
            },
            recv(timer) -> _ => {
                if seed_retry.is_some() && worker.event_queue_url.is_none() {
                    worker.retry_seed(map_tx);
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{
        EqCommand, EventQueueWorker, MAX_SEED_FETCH_RETRIES, SeedOutcome, drive_event_queue,
    };
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener, TcpStream};
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
            requested: std::sync::Arc::from(Vec::<String>::new()),
            in_flight: HashSet::new(),
            generation: 0,
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

    /// A long poll as the stub grid holds it: which queue, and the handle whose
    /// drop lets it return.
    type HeldPoll = (String, Sender<()>);

    /// How long a poll that should arrive is given to do so.
    const POLL_ARRIVES: Duration = Duration::from_secs(5);

    /// Answers one request: a seed with that region's queue, or a long poll
    /// that stays open until its [`HeldPoll`] handle is dropped.
    fn serve_connection(mut stream: TcpStream, addr: SocketAddr, polls: &Sender<HeldPoll>) {
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        let body_read = loop {
            let read = stream.read(&mut chunk).expect("the request is readable");
            request.extend(chunk.iter().take(read));
            let text = String::from_utf8_lossy(&request);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if body.len() >= length {
                    break head.to_owned();
                }
            }
            assert!(read > 0, "the client hung up mid-request");
        };
        let path = body_read
            .split_whitespace()
            .nth(1)
            .expect("the request names a path")
            .to_owned();
        let reply = if let Some(region) = path.strip_prefix("/seed/") {
            let body = format!(
                "<llsd><map><key>EventQueueGet</key>\
                 <string>http://{addr}/eq/{region}</string></map></llsd>"
            );
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        } else {
            let region = path.trim_start_matches("/eq/").to_owned();
            let (release_tx, release_rx) = bounded::<()>(0);
            if polls.send((region, release_tx)).is_err() {
                return;
            }
            // Returns when the test drops the handle.
            let _released = release_rx.recv();
            "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
        };
        // The client may be gone by the time a held poll is released.
        let _ignored = stream.write_all(reply.as_bytes());
    }

    /// A stub grid on a loopback port, and the event-queue worker running
    /// against its region `a`. The client bypasses the process-global proxy a
    /// sibling test installs.
    fn stub_grid() -> (SocketAddr, Sender<EqCommand>, Receiver<HeldPoll>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let addr = listener.local_addr().expect("the port has an address");
        let (polls_tx, polls_rx) = unbounded();
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let polls = polls_tx.clone();
                std::thread::spawn(move || serve_connection(stream, addr, &polls));
            }
        });
        let (command_tx, command_rx) = unbounded();
        let (caps_tx, caps_rx) = unbounded();
        let (map_tx, map_rx) = unbounded();
        std::thread::spawn(move || {
            let http = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("a direct blocking client builds");
            let seed = seed_url(addr, "a");
            let requested = std::sync::Arc::from(Vec::<String>::new());
            let worker = EventQueueWorker::with_client(http, seed, requested, &map_tx);
            drive_event_queue(worker, &command_rx, &caps_tx, &map_tx);
            drop(caps_rx);
            drop(map_rx);
        });
        (addr, command_tx, polls_rx)
    }

    /// The seed capability of the stub grid's `region`.
    fn seed_url(addr: SocketAddr, region: &str) -> url::Url {
        format!("http://{addr}/seed/{region}")
            .parse()
            .expect("the stub seed URL parses")
    }

    /// A region switch takes effect while the simulator still holds the old
    /// region's long poll open.
    #[test]
    fn a_switch_does_not_wait_for_the_old_long_poll() {
        let (addr, commands, polls) = stub_grid();
        let (first, _hold_a) = polls
            .recv_timeout(POLL_ARRIVES)
            .expect("region a is polled");
        assert_eq!(first, "a");

        commands
            .send(EqCommand::Switch(seed_url(addr, "b")))
            .expect("the worker is running");
        let (second, _hold_b) = polls
            .recv_timeout(POLL_ARRIVES)
            .expect("region b is polled while a's poll is still open");
        assert_eq!(second, "b");
    }

    /// Coming back to a region whose abandoned poll is still open must not put
    /// a second poller on that queue.
    #[test]
    fn a_revisit_waits_for_the_poll_it_abandoned() {
        let (addr, commands, polls) = stub_grid();
        let (_first, hold_a) = polls
            .recv_timeout(POLL_ARRIVES)
            .expect("region a is polled");
        commands
            .send(EqCommand::Switch(seed_url(addr, "b")))
            .expect("the worker is running");
        let (_second, _hold_b) = polls
            .recv_timeout(POLL_ARRIVES)
            .expect("region b is polled");

        commands
            .send(EqCommand::Switch(seed_url(addr, "a")))
            .expect("the worker is running");
        assert!(
            polls.recv_timeout(Duration::from_millis(500)).is_err(),
            "a's queue still has an open poll, so it must not get a second one"
        );

        drop(hold_a);
        let (again, _hold_again) = polls
            .recv_timeout(POLL_ARRIVES)
            .expect("region a is polled again once its old poll has returned");
        assert_eq!(again, "a");
    }
}
