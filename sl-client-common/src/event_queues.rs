//! The event queues of every region a session holds, as a sans-IO plan.
//!
//! The reference viewer polls the `EventQueueGet` capability of **every**
//! region it holds, not just the one the agent is in: `LLViewerRegion` starts
//! an `LLEventPoll` as soon as a region's capabilities name the queue, for a
//! neighbour as much as for the root, and stops it only when the region is
//! removed. A region the agent left by teleport is removed by its
//! `DisableSimulator`, about 50 s later on Second Life. Polling only the
//! root's queue left Second Life's neighbours repeating
//! `EstablishAgentCommunication` every five seconds. A region left for a
//! minute then handed the agent its revoked capabilities on the way back.
//!
//! [`EventQueues`] holds what both runtimes need to do the same: each region's
//! seed, capability map, queue URL and ack, and the retry budget of its seed
//! fetch. It does no I/O. The driver tells it what the session holds and what
//! each request returned, and runs the [`Action`]s it hands back: a seed POST,
//! one long poll, or a capability map to install. The events a poll returns go
//! to [`Session::handle_caps_event_from`](sl_proto::Session::handle_caps_event_from)
//! with the region they came from.
//!
//! At most one long poll is open on a queue at any time, also across a region
//! being dropped and held again. Two polls on one queue corrupt
//! `EventQueueGet`'s ack-sequenced id stream and lose events (a lost
//! `CrossedRegion` froze the avatar). A queue is never closed with
//! `done: true`: on OpenSim that request blocks like a long poll. A poll still
//! open on a region that was dropped returns on its own, and its events still
//! go to the session.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;

use crate::retry::{MAX_TRANSIENT_RETRIES, transient_backoff};

/// A region's capability map: capability name to URL.
pub type CapabilityMap = HashMap<String, String>;

/// The name of the event-queue capability.
const EVENT_QUEUE_GET: &str = "EventQueueGet";

/// How many times a failed seed fetch is retried before the region is left
/// without its capabilities. Shares the asset fetchers' transient-error
/// budget: the seed POST fails for the same reasons (a simulator still
/// spinning up a region's capability handlers answers a transient error before
/// it answers the map).
pub const MAX_SEED_FETCH_RETRIES: u32 = MAX_TRANSIENT_RETRIES;

/// Something the driver is to do for [`EventQueues`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// POST `seed` for the region at `sim` with the requested capability names,
    /// and report what it answered through [`EventQueues::seed_fetched`].
    FetchSeed {
        /// The region's simulator.
        sim: SocketAddr,
        /// Its seed capability.
        seed: url::Url,
    },
    /// Run one `EventQueueGet` long poll on `url` with `ack`, hand its events
    /// to the session as the region at `sim`'s, and report its batch id
    /// through [`EventQueues::poll_finished`].
    Poll {
        /// The region whose queue it is.
        sim: SocketAddr,
        /// The queue.
        url: String,
        /// The id of the last batch taken from it, if any.
        ack: Option<i32>,
    },
    /// Install this as the root region's capability map, or report why the
    /// root region has none. A failure comes after the retry budget is spent.
    RootMap(Result<CapabilityMap, String>),
    /// A neighbouring region's capability map, for the requests about its
    /// objects, or why it could not be had (after the retry budget).
    NeighbourMap {
        /// The neighbour's simulator.
        sim: SocketAddr,
        /// Its map, or the reason there is none.
        outcome: Result<CapabilityMap, String>,
    },
}

/// Where a region's seed fetch stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeedFetch {
    /// Not yet asked for.
    Due,
    /// A POST is out.
    InFlight,
    /// A POST failed; the next one goes out at this time.
    RetryAt(Instant),
    /// The region answered with its map.
    Done,
    /// Every attempt failed.
    GaveUp,
}

/// One held region's queue.
#[derive(Debug)]
struct RegionQueue {
    /// The seed capability its map comes from.
    seed: url::Url,
    /// The map, once fetched.
    map: Option<CapabilityMap>,
    /// Its `EventQueueGet` URL, once fetched, if it has one.
    queue: Option<String>,
    /// The id of the last batch taken from its queue.
    ack: Option<i32>,
    /// Where its seed fetch stands.
    fetch: SeedFetch,
    /// How many seed fetches have failed in a row.
    failures: u32,
    /// Whether it was announced as a neighbour, so a map fetched while it is
    /// not the root is handed out as a neighbour's. A teleport's destination
    /// is not one: its map is handed out once it becomes the root.
    neighbour: bool,
}

impl RegionQueue {
    /// A region whose seed is yet to be fetched.
    const fn new(seed: url::Url, neighbour: bool) -> Self {
        Self {
            seed,
            map: None,
            queue: None,
            ack: None,
            fetch: SeedFetch::Due,
            failures: 0,
            neighbour,
        }
    }
}

/// The event queues of every region a session holds. See the
/// [module documentation](self).
#[derive(Debug, Default)]
pub struct EventQueues {
    /// Every held region, by simulator address.
    regions: BTreeMap<SocketAddr, RegionQueue>,
    /// The root region's simulator.
    root: Option<SocketAddr>,
    /// Every queue with a long poll open, including queues of regions no
    /// longer held. A queue listed here is not polled again until its poll
    /// returns.
    polling: HashSet<String>,
    /// Capability maps to hand out with the next actions.
    maps: VecDeque<Action>,
}

impl EventQueues {
    /// No regions yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The region at `sim` is the root, reached through `seed`. Its capability
    /// map is handed out as [`Action::RootMap`]: at once when the region is
    /// already held with this seed and its map fetched (a neighbour crossed
    /// into, or a region returned to before it disabled the agent), which
    /// keeps polling its queue without fetching the seed again, as the
    /// reference ignores a duplicate seed. A different seed starts the region
    /// afresh.
    pub fn set_root(&mut self, sim: SocketAddr, seed: url::Url) {
        self.root = Some(sim);
        match self.regions.get_mut(&sim) {
            Some(region) if region.seed == seed => match region.fetch {
                SeedFetch::Done => {
                    if let Some(map) = &region.map {
                        self.maps.push_back(Action::RootMap(Ok(map.clone())));
                    }
                }
                SeedFetch::GaveUp => {
                    region.fetch = SeedFetch::Due;
                    region.failures = 0;
                }
                SeedFetch::Due | SeedFetch::InFlight | SeedFetch::RetryAt(_) => {}
            },
            Some(_) | None => {
                let _replaced = self.regions.insert(sim, RegionQueue::new(seed, false));
            }
        }
    }

    /// The region at `sim` is the root, reached through `seed`, and its map was
    /// already fetched by the driver (the login's own fetch). Nothing is handed
    /// out for it; its queue is polled from the next actions.
    pub fn set_root_fetched(&mut self, sim: SocketAddr, seed: url::Url, map: CapabilityMap) {
        self.root = Some(sim);
        let mut region = RegionQueue::new(seed, false);
        region.queue = map.get(EVENT_QUEUE_GET).cloned();
        region.map = Some(map);
        region.fetch = SeedFetch::Done;
        let _replaced = self.regions.insert(sim, region);
    }

    /// A neighbouring region at `sim` named its seed. A new region, or one with
    /// a new seed, has the seed fetched and its queue polled. One already held
    /// with this seed hands out its map again, if fetched.
    pub fn add_neighbour(&mut self, sim: SocketAddr, seed: url::Url) {
        self.add_region(sim, seed, true);
    }

    /// A teleport's `TeleportFinish` named the region at `sim` as its
    /// destination, with `seed`. The seed is fetched and the queue polled at
    /// once, before the destination confirms the arrival, as the reference
    /// viewer does. Its map is handed out when it becomes the root
    /// ([`Self::set_root`]).
    pub fn add_destination(&mut self, sim: SocketAddr, seed: url::Url) {
        self.add_region(sim, seed, false);
    }

    /// Holds the region at `sim`, reached through `seed`.
    fn add_region(&mut self, sim: SocketAddr, seed: url::Url, neighbour: bool) {
        let is_root = self.root == Some(sim);
        match self.regions.get_mut(&sim) {
            Some(region) if region.seed == seed => {
                region.neighbour |= neighbour;
                match region.fetch {
                    SeedFetch::Done => {
                        if let Some(map) = region.map.as_ref().filter(|_| neighbour && !is_root) {
                            self.maps.push_back(Action::NeighbourMap {
                                sim,
                                outcome: Ok(map.clone()),
                            });
                        }
                    }
                    SeedFetch::GaveUp => {
                        region.fetch = SeedFetch::Due;
                        region.failures = 0;
                    }
                    SeedFetch::Due | SeedFetch::InFlight | SeedFetch::RetryAt(_) => {}
                }
            }
            Some(_) | None => {
                let _replaced = self.regions.insert(sim, RegionQueue::new(seed, neighbour));
            }
        }
    }

    /// Drops every region `holds` says the session no longer holds. Its queue
    /// is not polled again; a poll still open on it returns on its own.
    pub fn retain(&mut self, mut holds: impl FnMut(SocketAddr) -> bool) {
        let root = self.root;
        self.regions.retain(|sim, region| {
            let held = Some(*sim) == root || holds(*sim);
            if !held {
                tracing::debug!(
                    %sim,
                    queue = region.queue.as_deref().unwrap_or("none"),
                    "event queue: no longer polling a region the session dropped"
                );
            }
            held
        });
    }

    /// Whether the region at `sim` is held.
    #[must_use]
    pub fn is_held(&self, sim: SocketAddr) -> bool {
        self.regions.contains_key(&sim)
    }

    /// The seed POST for the region at `sim` answered `outcome`. An answer for a
    /// seed the region no longer has (a region dropped, or started afresh, while
    /// the POST was out) is ignored.
    pub fn seed_fetched(
        &mut self,
        sim: SocketAddr,
        seed: &url::Url,
        outcome: Result<CapabilityMap, String>,
        now: Instant,
    ) {
        let is_root = self.root == Some(sim);
        let Some(region) = self
            .regions
            .get_mut(&sim)
            .filter(|region| region.seed == *seed && region.fetch == SeedFetch::InFlight)
        else {
            return;
        };
        match outcome {
            Ok(map) => {
                region.queue = map.get(EVENT_QUEUE_GET).cloned();
                if region.queue.is_none() {
                    tracing::warn!(
                        %sim,
                        %seed,
                        capabilities = map.len(),
                        "event queue: the region advertises no EventQueueGet"
                    );
                }
                region.fetch = SeedFetch::Done;
                region.failures = 0;
                region.map = Some(map.clone());
                if is_root {
                    self.maps.push_back(Action::RootMap(Ok(map)));
                } else if region.neighbour {
                    self.maps.push_back(Action::NeighbourMap {
                        sim,
                        outcome: Ok(map),
                    });
                }
            }
            Err(reason) => {
                region.failures = region.failures.saturating_add(1);
                tracing::warn!(
                    %sim,
                    %seed,
                    failures = region.failures,
                    "event queue: the seed-capabilities fetch failed: {reason}"
                );
                if region.failures > MAX_SEED_FETCH_RETRIES {
                    region.fetch = SeedFetch::GaveUp;
                    if is_root {
                        self.maps.push_back(Action::RootMap(Err(reason)));
                    } else if region.neighbour {
                        self.maps.push_back(Action::NeighbourMap {
                            sim,
                            outcome: Err(reason),
                        });
                    }
                } else {
                    let backoff = transient_backoff(region.failures.saturating_sub(1));
                    region.fetch = SeedFetch::RetryAt(now.checked_add(backoff).unwrap_or(now));
                }
            }
        }
    }

    /// A long poll on `url`, the queue of the region at `sim`, returned.
    /// `batch` is the id of the batch it carried, `None` for a round that
    /// carried none (an idle timeout or a transient error). The queue may be
    /// polled again from the next actions.
    pub fn poll_finished(&mut self, sim: SocketAddr, url: &str, batch: Option<i32>) {
        let _was_open = self.polling.remove(url);
        if let Some(id) = batch
            && let Some(region) = self.regions.get_mut(&sim)
            && region.queue.as_deref() == Some(url)
        {
            region.ack = Some(id);
        }
    }

    /// What the driver is to do now: capability maps to install, seed POSTs
    /// that are due, and a long poll on every queue that has none open.
    pub fn take_actions(&mut self, now: Instant) -> Vec<Action> {
        let mut actions: Vec<Action> = self.maps.drain(..).collect();
        for (sim, region) in &mut self.regions {
            let due = match region.fetch {
                SeedFetch::Due => true,
                SeedFetch::RetryAt(at) => now >= at,
                SeedFetch::InFlight | SeedFetch::Done | SeedFetch::GaveUp => false,
            };
            if due {
                region.fetch = SeedFetch::InFlight;
                actions.push(Action::FetchSeed {
                    sim: *sim,
                    seed: region.seed.clone(),
                });
            }
            if let Some(url) = &region.queue
                && self.polling.insert(url.clone())
            {
                actions.push(Action::Poll {
                    sim: *sim,
                    url: url.clone(),
                    ack: region.ack,
                });
            }
        }
        actions
    }

    /// When a failed seed fetch is next due, if one is waiting.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.regions
            .values()
            .filter_map(|region| match region.fetch {
                SeedFetch::RetryAt(at) => Some(at),
                SeedFetch::Due | SeedFetch::InFlight | SeedFetch::Done | SeedFetch::GaveUp => None,
            })
            .min()
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{Action, CapabilityMap, EventQueues, MAX_SEED_FETCH_RETRIES};
    use pretty_assertions::assert_eq;
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    /// A simulator address on port `port`.
    fn sim(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// The seed of region `name`.
    fn seed(name: &str) -> url::Url {
        format!("http://grid.example/seed/{name}")
            .parse()
            .expect("the seed URL parses")
    }

    /// The queue URL of region `name`.
    fn queue(name: &str) -> String {
        format!("http://grid.example/eq/{name}")
    }

    /// The capability map region `name` answers with.
    fn map(name: &str) -> CapabilityMap {
        CapabilityMap::from([("EventQueueGet".to_owned(), queue(name))])
    }

    /// Answers every due seed fetch with its region's map.
    fn answer_seeds(queues: &mut EventQueues, actions: &[Action], now: Instant) {
        for action in actions {
            if let Action::FetchSeed { sim, seed } = action {
                let name = seed
                    .path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .expect("the seed names its region")
                    .to_owned();
                queues.seed_fetched(*sim, seed, Ok(map(&name)), now);
            }
        }
    }

    /// The queues the actions open a poll on.
    fn polls(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|action| match action {
                Action::Poll { url, .. } => Some(url.clone()),
                Action::FetchSeed { .. } | Action::RootMap(_) | Action::NeighbourMap { .. } => None,
            })
            .collect()
    }

    /// The root and a neighbour both have their seeds fetched and their queues
    /// polled, and each map goes where it belongs.
    #[test]
    fn the_root_and_a_neighbour_are_both_polled() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root(sim(1), seed("a"));
        queues.add_neighbour(sim(2), seed("b"));
        let fetches = queues.take_actions(now);
        assert_eq!(
            fetches,
            [
                Action::FetchSeed {
                    sim: sim(1),
                    seed: seed("a")
                },
                Action::FetchSeed {
                    sim: sim(2),
                    seed: seed("b")
                },
            ]
        );
        answer_seeds(&mut queues, &fetches, now);
        let actions = queues.take_actions(now);
        assert_eq!(
            actions,
            [
                Action::RootMap(Ok(map("a"))),
                Action::NeighbourMap {
                    sim: sim(2),
                    outcome: Ok(map("b"))
                },
                Action::Poll {
                    sim: sim(1),
                    url: queue("a"),
                    ack: None
                },
                Action::Poll {
                    sim: sim(2),
                    url: queue("b"),
                    ack: None
                },
            ]
        );
        assert!(
            queues.take_actions(now).is_empty(),
            "a queue with a poll open is not polled twice"
        );
    }

    /// A returned poll acks its batch on the next poll of the same queue.
    #[test]
    fn the_next_poll_acks_the_last_batch() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root_fetched(sim(1), seed("a"), map("a"));
        assert_eq!(polls(&queues.take_actions(now)), [queue("a")]);
        queues.poll_finished(sim(1), &queue("a"), Some(7));
        assert_eq!(
            queues.take_actions(now),
            [Action::Poll {
                sim: sim(1),
                url: queue("a"),
                ack: Some(7)
            }]
        );
        // An idle round keeps the ack.
        queues.poll_finished(sim(1), &queue("a"), None);
        assert_eq!(
            queues.take_actions(now),
            [Action::Poll {
                sim: sim(1),
                url: queue("a"),
                ack: Some(7)
            }]
        );
    }

    /// Crossing into a neighbour already polled makes it the root without a
    /// second seed POST, and the region left goes on being polled while the
    /// session holds it.
    #[test]
    fn a_crossing_keeps_both_queues_without_refetching() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root_fetched(sim(1), seed("a"), map("a"));
        queues.add_neighbour(sim(2), seed("b"));
        let actions = queues.take_actions(now);
        answer_seeds(&mut queues, &actions, now);
        let _maps_and_polls = queues.take_actions(now);

        queues.set_root(sim(2), seed("b"));
        assert_eq!(
            queues.take_actions(now),
            [Action::RootMap(Ok(map("b")))],
            "the map is handed out again as the root's, and no poll is doubled"
        );
        queues.retain(|held| held == sim(1));
        assert!(queues.is_held(sim(1)) && queues.is_held(sim(2)));
    }

    /// A region the session dropped is not polled again, but its open poll
    /// still blocks a second one if the region is held again before it
    /// returns, and the batch that poll delivered is acked by the next.
    #[test]
    fn a_dropped_region_is_not_polled_twice_when_held_again() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root_fetched(sim(1), seed("a"), map("a"));
        queues.add_neighbour(sim(2), seed("b"));
        let actions = queues.take_actions(now);
        answer_seeds(&mut queues, &actions, now);
        let _maps_and_polls = queues.take_actions(now);

        queues.retain(|held| held != sim(2));
        assert!(!queues.is_held(sim(2)));
        queues.add_neighbour(sim(2), seed("b"));
        let actions = queues.take_actions(now);
        answer_seeds(&mut queues, &actions, now);
        assert!(
            polls(&queues.take_actions(now)).is_empty(),
            "b's old poll is still open"
        );
        queues.poll_finished(sim(2), &queue("b"), Some(3));
        assert_eq!(
            queues.take_actions(now),
            [Action::Poll {
                sim: sim(2),
                url: queue("b"),
                ack: Some(3)
            }],
            "the batch the old poll delivered is acked on the same queue"
        );
    }

    /// A failed seed fetch is retried on a lengthening backoff; once the budget
    /// is spent the failure is handed out once.
    #[test]
    fn a_failing_seed_backs_off_then_reports_once() {
        let start = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root(sim(1), seed("a"));
        let mut now = start;
        let mut fetches = 0;
        let mut failures = Vec::new();
        for _round in 0..(MAX_SEED_FETCH_RETRIES + 5) {
            for action in queues.take_actions(now) {
                match action {
                    Action::FetchSeed { sim, seed } => {
                        fetches += 1;
                        queues.seed_fetched(sim, &seed, Err("refused".to_owned()), now);
                    }
                    Action::RootMap(outcome) => failures.push(outcome),
                    Action::Poll { .. } | Action::NeighbourMap { .. } => {}
                }
            }
            now = queues.next_deadline().unwrap_or_else(|| {
                now.checked_add(Duration::from_secs(60))
                    .expect("a minute on is representable")
            });
        }
        assert_eq!(fetches, MAX_SEED_FETCH_RETRIES + 1);
        assert_eq!(failures, [Err("refused".to_owned())]);
        assert_eq!(queues.next_deadline(), None);
    }

    /// The first retry waits the shortest backoff, and nothing is due before.
    #[test]
    fn a_retry_waits_for_its_backoff() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.add_neighbour(sim(2), seed("b"));
        let _fetch = queues.take_actions(now);
        queues.seed_fetched(sim(2), &seed("b"), Err("busy".to_owned()), now);
        let due = queues.next_deadline().expect("a retry is scheduled");
        assert_eq!(Some(due), now.checked_add(Duration::from_millis(200)));
        assert!(queues.take_actions(now).is_empty());
        assert_eq!(
            queues.take_actions(due),
            [Action::FetchSeed {
                sim: sim(2),
                seed: seed("b")
            }]
        );
    }

    /// A teleport's destination is fetched and polled as soon as its finish
    /// arrives, its map is handed out only once it is the root, and its seed
    /// is not fetched a second time then.
    #[test]
    fn a_destination_is_polled_before_it_becomes_the_root() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root_fetched(sim(1), seed("a"), map("a"));
        let _poll_a = queues.take_actions(now);
        queues.add_destination(sim(3), seed("c"));
        let fetch = queues.take_actions(now);
        assert_eq!(
            fetch,
            [Action::FetchSeed {
                sim: sim(3),
                seed: seed("c")
            }]
        );
        answer_seeds(&mut queues, &fetch, now);
        assert_eq!(
            queues.take_actions(now),
            [Action::Poll {
                sim: sim(3),
                url: queue("c"),
                ack: None
            }],
            "the destination's map is not a neighbour's"
        );
        queues.set_root(sim(3), seed("c"));
        assert_eq!(queues.take_actions(now), [Action::RootMap(Ok(map("c")))]);
    }

    /// An answer for a seed the region no longer has is ignored.
    #[test]
    fn a_stale_seed_answer_is_ignored() {
        let now = Instant::now();
        let mut queues = EventQueues::new();
        queues.set_root(sim(1), seed("a"));
        let _fetch = queues.take_actions(now);
        queues.set_root(sim(1), seed("a2"));
        queues.seed_fetched(sim(1), &seed("a"), Ok(map("a")), now);
        assert_eq!(
            queues.take_actions(now),
            [Action::FetchSeed {
                sim: sim(1),
                seed: seed("a2")
            }]
        );
    }
}
