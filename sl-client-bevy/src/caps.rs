//! CAPS subsystem lifecycle: seed fetches and the `EventQueueGet` long polls of
//! every region the session holds.
//!
//! What to fetch and poll is planned by the runtime-independent
//! [`EventQueues`]; this module carries the plan out on threads. Each seed POST
//! and each long poll runs on a thread of its own and reports back over a
//! channel, which the network thread drains every tick
//! ([`Caps::pump`]). The events a poll returns go to
//! [`Session::handle_caps_event_from`](sl_proto::Session::handle_caps_event_from)
//! with the region they came from.
//!
//! The reference viewer polls the queue of every region it holds, the
//! neighbours and a region left by teleport (until it disables the agent)
//! included, so the runner does too. At most one poll is open on a queue at a
//! time, and a queue is never closed with `done: true`, which on OpenSim
//! blocks like a long poll.

use crate::{Caps, EVENT_QUEUE_TIMEOUT, deliver};
use crossbeam_channel::{Receiver, Sender, unbounded};
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_client_common::event_queues::{Action, CapabilityMap, EventQueues, MAX_SEED_FETCH_RETRIES};
use sl_client_common::retry::transient_backoff;
use sl_proto::{
    Llsd, Session, build_event_queue_request, build_seed_request, parse_event_queue_response,
    parse_seed_response,
};
use std::collections::HashMap;
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

/// Starts the CAPS subsystem for the session's root region: the event-queue
/// runner, which fetches the root's capability map (reported over `map_rx`)
/// and polls its queue, and later every other region's. Returns `None` if no
/// seed or no root circuit is known yet.
pub(crate) fn start_caps(session: &Session, requested: &Arc<[String]>) -> Option<Caps> {
    let Some(seed) = session.seed_capability().map(url::Url::to_owned) else {
        tracing::warn!("start_caps: no seed capability yet — event queue NOT started");
        return None;
    };
    let Some(root) = session.root_simulator() else {
        tracing::warn!("start_caps: no root circuit yet — event queue NOT started");
        return None;
    };
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (chat_replies_tx, chat_replies_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let (neighbour_map_tx, neighbour_map_rx) = unbounded();
    let http = match crate::http_proxy::blocking_client_builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    {
        Ok(http) => Some(http),
        Err(error) => {
            deliver(
                &map_tx,
                Err(format!("could not build the caps HTTP client: {error}")),
            );
            None
        }
    };
    let mut runner = http.map(|http| EventQueueRunner::new(http, Arc::clone(requested)));
    if let Some(runner) = runner.as_mut() {
        tracing::info!(%seed, %root, "start_caps: polling the root region's event queue");
        runner.queues.set_root(root, seed);
    }
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        chat_replies_rx,
        chat_replies_tx,
        map_rx,
        map_tx,
        map: HashMap::new(),
        neighbour_map_rx,
        neighbour_map_tx,
        runner,
        requested: Arc::clone(requested),
    })
}

impl Caps {
    /// The root region changed: its queue is polled (without a second seed
    /// POST when the region is already held with this seed) and its map
    /// installed. The region left goes on being polled while the session
    /// holds it.
    pub(crate) fn set_root(&mut self, session: &Session) {
        let (Some(seed), Some(root)) = (session.seed_capability(), session.root_simulator()) else {
            tracing::warn!(
                "a region change with no seed or no root circuit: its event queue is not polled"
            );
            return;
        };
        if let Some(runner) = self.runner.as_mut() {
            tracing::info!(%seed, %root, "event queue: the root region changed");
            runner.queues.set_root(root, seed.to_owned());
        }
    }

    /// A neighbour named its seed: POST it, so the simulator streams that
    /// region's scene to the child circuit (its `SendInitialData` is gated on
    /// the seed having been requested), keep the map for requests about its
    /// objects, and poll its queue.
    pub(crate) fn add_neighbour(&mut self, sim: SocketAddr, seed: url::Url) {
        match self.runner.as_mut() {
            Some(runner) => runner.queues.add_neighbour(sim, seed),
            None => fetch_neighbour_caps(
                sim,
                seed,
                Arc::clone(&self.requested),
                self.neighbour_map_tx.clone(),
            ),
        }
    }

    /// A teleport's finish named its destination: POST the destination's seed
    /// and poll its queue now, before the arrival is confirmed, as the
    /// reference does right after `CompleteAgentMovement`.
    pub(crate) fn add_destination(&mut self, session: &Session) {
        let Some((sim, seed)) = session.handover_destination() else {
            return;
        };
        if let Some(runner) = self.runner.as_mut() {
            tracing::debug!(%sim, %seed, "event queue: fetching a teleport destination's seed");
            runner.queues.add_destination(sim, seed.to_owned());
        }
    }

    /// Takes in what the seed fetches and long polls returned, hands each
    /// event to the session as its region's, stops polling the regions the
    /// session no longer holds, and starts whatever is due. Called every
    /// network tick.
    pub(crate) fn pump(&mut self, session: &mut Session, now: Instant) {
        let Some(runner) = self.runner.as_mut() else {
            return;
        };
        runner.take_in(now, |sim, message, body| {
            if let Err(error) = session.handle_caps_event_from(sim, &message, &body, now) {
                tracing::warn!(event = %message, %sim, "event-queue event rejected: {error}");
            }
        });
        runner.dispatch(
            now,
            |sim| session.holds_region(sim),
            &self.map_tx,
            &self.neighbour_map_tx,
        );
    }
}

/// A neighbouring region's capability map, or why it could not be fetched,
/// keyed by the neighbour's simulator address.
pub(crate) type NeighbourMapOutcome = (SocketAddr, Result<HashMap<String, String>, String>);

/// POSTs a neighbour region's seed capability on a detached thread and reports
/// its map over `map_tx`. Used only without an event-queue runner (no HTTP
/// client could be built, or no CAPS subsystem at all), so the neighbour's
/// scene still streams.
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

/// A decoded `EventQueueGet` reply: its batch id and its `(message, body)`
/// events.
type PollReply = (i32, Vec<(String, Llsd)>);

/// A seed POST that has returned.
struct FinishedSeed {
    /// The region it was for.
    sim: SocketAddr,
    /// The seed it POSTed.
    seed: url::Url,
    /// The map, or why there is none.
    outcome: Result<CapabilityMap, String>,
}

/// A long poll that has returned.
struct FinishedPoll {
    /// The region whose queue it polled.
    sim: SocketAddr,
    /// The queue.
    url: String,
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

/// Carries out the [`EventQueues`] plan: a thread per seed POST and per long
/// poll, each reporting back over a channel that [`Self::pump`] drains.
pub(crate) struct EventQueueRunner {
    /// The plan.
    queues: EventQueues,
    /// The blocking HTTP client the seed POSTs and the long polls share.
    http: ReqwestBlockingClient,
    /// The capability names every seed POST asks for.
    requested: Arc<[String]>,
    /// Where a seed POST thread reports.
    seed_tx: Sender<FinishedSeed>,
    /// What the seed POST threads reported.
    seed_rx: Receiver<FinishedSeed>,
    /// Where a long-poll thread reports.
    poll_tx: Sender<FinishedPoll>,
    /// What the long-poll threads reported.
    poll_rx: Receiver<FinishedPoll>,
}

impl EventQueueRunner {
    /// A runner with no regions yet, on the HTTP client `http`.
    fn new(http: ReqwestBlockingClient, requested: Arc<[String]>) -> Self {
        let (seed_tx, seed_rx) = unbounded();
        let (poll_tx, poll_rx) = unbounded();
        Self {
            queues: EventQueues::new(),
            http,
            requested,
            seed_tx,
            seed_rx,
            poll_tx,
            poll_rx,
        }
    }

    /// Takes in every returned seed POST and long poll, handing each polled
    /// event to `deliver_event` with its region.
    fn take_in(&mut self, now: Instant, mut deliver_event: impl FnMut(SocketAddr, String, Llsd)) {
        while let Ok(FinishedSeed { sim, seed, outcome }) = self.seed_rx.try_recv() {
            self.queues.seed_fetched(sim, &seed, outcome, now);
        }
        while let Ok(FinishedPoll { sim, url, reply }) = self.poll_rx.try_recv() {
            let batch = reply.as_ref().map(|(id, _events)| *id);
            if let Some((id, events)) = reply
                && !events.is_empty()
            {
                let names: Vec<&str> = events.iter().map(|(name, _body)| name.as_str()).collect();
                tracing::debug!(id, %sim, ?names, "event queue: delivering batch");
                for (message, body) in events {
                    deliver_event(sim, message, body);
                }
            }
            self.queues.poll_finished(sim, &url, batch);
        }
    }

    /// Drops the regions `holds` says are gone and starts every seed POST and
    /// poll now due. A root map goes over `map_tx`, a neighbour's over
    /// `neighbour_map_tx`.
    fn dispatch(
        &mut self,
        now: Instant,
        holds: impl FnMut(SocketAddr) -> bool,
        map_tx: &Sender<Result<CapabilityMap, String>>,
        neighbour_map_tx: &Sender<NeighbourMapOutcome>,
    ) {
        self.queues.retain(holds);
        for action in self.queues.take_actions(now) {
            match action {
                Action::FetchSeed { sim, seed } => self.spawn_seed(sim, seed),
                Action::Poll { sim, url, ack } => self.spawn_poll(sim, url, ack),
                Action::RootMap(outcome) => deliver(map_tx, outcome),
                Action::NeighbourMap { sim, outcome } => deliver(neighbour_map_tx, (sim, outcome)),
            }
        }
    }

    /// POSTs `seed` for the region at `sim` on a thread of its own.
    fn spawn_seed(&self, sim: SocketAddr, seed: url::Url) {
        let http = self.http.clone();
        let requested = Arc::clone(&self.requested);
        let seed_tx = self.seed_tx.clone();
        crate::log_context::spawn_thread(move || {
            let outcome = post_seed(&http, &seed, &requested);
            deliver(&seed_tx, FinishedSeed { sim, seed, outcome });
        });
    }

    /// Runs one long poll on `url` on a thread of its own.
    fn spawn_poll(&self, sim: SocketAddr, url: String, ack: Option<i32>) {
        let http = self.http.clone();
        let poll_tx = self.poll_tx.clone();
        crate::log_context::spawn_thread(move || {
            let reply = fetch_poll(&http, &url, ack);
            deliver(&poll_tx, FinishedPoll { sim, url, reply });
        });
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::EventQueueRunner;
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::time::{Duration, Instant};

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

    /// A stub grid on a loopback port.
    fn stub_grid() -> (SocketAddr, Receiver<HeldPoll>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let addr = listener.local_addr().expect("the port has an address");
        let (polls_tx, polls_rx) = unbounded();
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let polls = polls_tx.clone();
                std::thread::spawn(move || serve_connection(stream, addr, &polls));
            }
        });
        (addr, polls_rx)
    }

    /// The seed capability of the stub grid's `region`.
    fn seed_url(addr: SocketAddr, region: &str) -> url::Url {
        format!("http://{addr}/seed/{region}")
            .parse()
            .expect("the stub seed URL parses")
    }

    /// A runner on a client that bypasses the process-global proxy a sibling
    /// test installs.
    fn runner() -> EventQueueRunner {
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("a direct blocking client builds");
        EventQueueRunner::new(http, std::sync::Arc::from(Vec::<String>::new()))
    }

    /// Pumps `runner` until `count` polls are held open by the stub grid,
    /// returning the regions they are on and their handles.
    fn pump_until_polled(
        runner: &mut EventQueueRunner,
        polls: &Receiver<HeldPoll>,
        count: usize,
    ) -> Vec<HeldPoll> {
        let (map_tx, _map_rx) = unbounded();
        let (neighbour_map_tx, _neighbour_map_rx) = unbounded();
        let deadline = Instant::now()
            .checked_add(POLL_ARRIVES)
            .expect("the deadline is representable");
        let mut held = Vec::new();
        while held.len() < count && Instant::now() < deadline {
            runner.take_in(Instant::now(), |_sim, _message, _body| {});
            runner.dispatch(Instant::now(), |_sim| true, &map_tx, &neighbour_map_tx);
            if let Ok(poll) = polls.recv_timeout(Duration::from_millis(20)) {
                held.push(poll);
            }
        }
        held
    }

    /// The root's queue and a neighbour's are polled at once, each held open
    /// by the simulator while the other is.
    #[test]
    fn the_root_and_a_neighbour_are_polled_at_once() {
        let (addr, polls) = stub_grid();
        let mut runner = runner();
        runner
            .queues
            .set_root(SocketAddr::from(([127, 0, 0, 1], 1)), seed_url(addr, "a"));
        runner
            .queues
            .add_neighbour(SocketAddr::from(([127, 0, 0, 1], 2)), seed_url(addr, "b"));
        let held = pump_until_polled(&mut runner, &polls, 2);
        let regions: HashSet<String> = held
            .iter()
            .map(|(region, _handle)| region.clone())
            .collect();
        assert_eq!(regions, HashSet::from(["a".to_owned(), "b".to_owned()]));
    }

    /// A queue with a poll held open is not polled a second time, and is
    /// polled again once that poll returns.
    #[test]
    fn a_queue_is_polled_again_only_once_its_poll_returns() {
        let (addr, polls) = stub_grid();
        let mut runner = runner();
        runner
            .queues
            .set_root(SocketAddr::from(([127, 0, 0, 1], 1)), seed_url(addr, "a"));
        let mut held = pump_until_polled(&mut runner, &polls, 1);
        assert_eq!(held.len(), 1);
        assert!(
            pump_until_polled(&mut runner, &polls, 1).is_empty(),
            "a's queue has a poll open, so it must not get a second one"
        );
        drop(held.pop());
        let again = pump_until_polled(&mut runner, &polls, 1);
        assert_eq!(
            again.first().map(|(region, _handle)| region.as_str()),
            Some("a")
        );
    }
}
