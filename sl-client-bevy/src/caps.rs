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

use crate::{Caps, EVENT_QUEUE_TIMEOUT};
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

/// Reports that a CAPS request for `cap` failed before producing a reply,
/// sending the failure sentinel over `caps_tx`. Helpers call this in place of
/// silently swallowing a transport / parse error; the driver turns it into a
/// diagnostic.
pub(crate) fn report_caps_failure(caps_tx: &Sender<(String, Llsd)>, cap: &str) {
    caps_tx
        .send((format!("{CAPS_FAILURE_PREFIX}{cap}"), Llsd::Undef))
        .ok();
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

/// POSTs `seed_url` to fetch a region's capability map (reporting it over
/// `map_tx`, `Ok` or a readable `Err`) and returns its `EventQueueGet` URL, or
/// `None` if the seed request / parse failed or the region advertises no event
/// queue.
fn fetch_caps(
    http: &ReqwestBlockingClient,
    seed_url: &url::Url,
    map_tx: &Sender<Result<HashMap<String, String>, String>>,
) -> Option<String> {
    let response = match http
        .post(seed_url.clone())
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%seed_url, %error, "event queue: seed-capabilities POST failed — no queue for this region");
            map_tx
                .send(Err(format!(
                    "the seed-capabilities request failed: {error}"
                )))
                .ok();
            return None;
        }
    };
    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            map_tx
                .send(Err(format!(
                    "the seed-capabilities response body could not be read: {error}"
                )))
                .ok();
            return None;
        }
    };
    let capabilities = match parse_seed_response(&text) {
        Ok(capabilities) => capabilities,
        Err(error) => {
            map_tx
                .send(Err(format!(
                    "the seed-capabilities response did not parse: {error}"
                )))
                .ok();
            return None;
        }
    };
    map_tx.send(Ok(capabilities.clone())).ok();
    let url = capabilities.get("EventQueueGet").cloned();
    if url.is_none() {
        tracing::warn!(
            %seed_url,
            caps = capabilities.len(),
            "event queue: region advertises NO EventQueueGet — no CrossedRegion / EnableSimulator will arrive"
        );
    }
    url
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
}

impl EventQueueWorker {
    /// Builds the worker for `seed`, fetching its capability map + event-queue
    /// URL. `None` if the HTTP client could not be built.
    fn new(
        seed: url::Url,
        map_tx: &Sender<Result<HashMap<String, String>, String>>,
    ) -> Option<Self> {
        let http = match ReqwestBlockingClient::builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        {
            Ok(http) => http,
            Err(error) => {
                map_tx
                    .send(Err(format!(
                        "could not build the caps HTTP client: {error}"
                    )))
                    .ok();
                return None;
            }
        };
        let event_queue_url = fetch_caps(&http, &seed, map_tx);
        if let Some(url) = &event_queue_url {
            tracing::info!(%url, "event queue: polling started");
        }
        Some(Self {
            http,
            seed,
            event_queue_url,
            ack: None,
        })
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
        self.event_queue_url = fetch_caps(&self.http, &self.seed, map_tx);
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
                std::thread::sleep(Duration::from_secs(1));
                return PollOutcome::Idle;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with the
        // same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            return PollOutcome::Idle;
        }
        let Ok(text) = response.text() else {
            return PollOutcome::Idle;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            return PollOutcome::Idle;
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
            // Nothing to poll (no seed / no EventQueueGet): block for a command so
            // the thread does not spin.
            match command_rx.recv() {
                Ok(EqCommand::Switch(seed)) => {
                    worker.switch(seed, map_tx);
                    continue;
                }
                Err(_) => return,
            }
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
