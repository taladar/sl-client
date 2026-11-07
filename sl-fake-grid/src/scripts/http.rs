//! Scripts' HTTP, the region's half (`server-lsl-lib-http-url`): the host
//! methods that grant URLs, carry requests and answers and keep each prim's
//! throttle, and the hand-overs a tick makes — requests and answers in before
//! the scripts run, what they sent routed after. The grid-wide registry and
//! responder are [`crate::script_http`].

use std::collections::BTreeMap;

use sl_lsl_runtime::vm::http::{self, Answered, Arrival, Outbound, Throttle};
use sl_lsl_runtime::vm::{CallerId, Engine, Tick};
use sl_types::key::ObjectKey;

use super::{RegionHost, RegionScripts};
use crate::runtime::GridCore;
use crate::script_http::{Arriving, Granted, Reply, Returning};

/// A region's HTTP: what its scripts sent and answered this tick, the
/// requests that reached them and wait for an answer, and each prim's
/// throttle.
#[derive(Debug, Default)]
pub(crate) struct RegionHttp {
    /// Requests the scripts sent this tick.
    sent: Vec<(CallerId, Outbound)>,
    /// Answers the scripts gave this tick: the request's key and the answer.
    answered: Vec<(String, Answered)>,
    /// Requests that reached a script and are not answered, by key: where the
    /// answer goes, the method and the tick it arrived.
    waiting: BTreeMap<String, (Reply, String, Tick)>,
    /// Each prim's `llHTTPRequest` throttle.
    throttles: BTreeMap<ObjectKey, Throttle>,
    /// How many requests have arrived, for their keys.
    arrivals: u64,
}

impl Clone for RegionHttp {
    /// A copy of the region's HTTP for a copy of the region: what was sent
    /// and answered and the throttles, but none of the requests waiting on an
    /// answer, which only the region they reached can give.
    fn clone(&self) -> Self {
        Self {
            sent: self.sent.clone(),
            answered: self.answered.clone(),
            waiting: BTreeMap::new(),
            throttles: self.throttles.clone(),
            arrivals: self.arrivals,
        }
    }
}

/// The `host:port` the grid's script URLs are reached at.
pub(crate) fn authority(core: &GridCore) -> String {
    format!("127.0.0.1:{}", core.http_port)
}

/// The address a request from a script of the grid arrives from.
const LOOPBACK: &str = "127.0.0.1";

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// `llHTTPRequest`: kept for the tick's end.
    pub(super) fn send_http_of(&mut self, caller: CallerId, request: &Outbound) {
        self.world
            .script_instances
            .http
            .sent
            .push((caller, request.clone()));
    }

    /// `llRequestURL`: a URL on the grid's HTTP service, unless the region's
    /// allowance is spent or a secure one is asked for — the fake grid runs
    /// no HTTPS listener, so it denies one on both flavours.
    pub(super) fn request_url_of(
        &self,
        caller: CallerId,
        key: &str,
        secure: bool,
    ) -> Option<String> {
        if secure || self.free_urls_of() <= 0 {
            return None;
        }
        let flavour = self.world.script_instances.flavour();
        let url = http::granted_url(flavour, &authority(self.core), key, false);
        let _previous = self.core.script_http.lock().urls.insert(
            key.to_owned(),
            Granted {
                region: self.index,
                caller,
                url: url.clone(),
                flavour,
            },
        );
        Some(url)
    }

    /// `llReleaseURL`.
    pub(super) fn release_url_of(&self, caller: CallerId, url: &str) {
        let index = self.index;
        self.core.script_http.lock().urls.retain(|_, granted| {
            !(granted.region == index && granted.caller == caller && granted.url == url)
        });
    }

    /// `llGetFreeURLs`: the region's allowance less what its scripts hold.
    pub(super) fn free_urls_of(&self) -> i32 {
        let flavour = self.world.script_instances.flavour();
        let prims = self
            .world
            .parcels
            .first()
            .map_or(15_000, |parcel| parcel.sim_wide_max_prims);
        let held = self.core.script_http.lock().held_in(self.index);
        crate::script_http::allowance(flavour, prims)
            .saturating_sub(i32::try_from(held).unwrap_or(i32::MAX))
    }

    /// `llHTTPResponse`: kept for the tick's end.
    pub(super) fn answer_http_of(&mut self, request: &str, answered: Answered) {
        self.world
            .script_instances
            .http
            .answered
            .push((request.to_owned(), answered));
    }

    /// The throttle of `caller`'s prim.
    pub(super) fn throttle_of(&mut self, caller: CallerId) -> &mut Throttle {
        let prim = self
            .origin(caller)
            .map_or_else(|| ObjectKey::from(uuid::Uuid::nil()), |origin| origin.prim);
        self.world
            .script_instances
            .http
            .throttles
            .entry(prim)
            .or_default()
    }
}

/// Before a tick's scripts run: the requests that arrived for the region's
/// scripts reach them, and the answers to theirs come back — on Second Life
/// to every script of the sender's prim.
pub(super) fn hand_in(core: &GridCore, index: usize, scripts: &mut RegionScripts) {
    let (arriving, returning) = {
        let mut shared = core.script_http.lock();
        (shared.take_arriving(index), shared.take_returning(index))
    };
    let now = scripts.engine.now();
    for Arriving {
        caller,
        inbound,
        reply,
        ..
    } in arriving
    {
        scripts.http.arrivals = scripts.http.arrivals.saturating_add(1);
        let id = request_key(index, scripts.http.arrivals);
        let method = inbound.method.clone();
        match scripts.engine.http_request(caller, &id, inbound) {
            Ok(Some(_)) => {
                let _previous = scripts.http.waiting.insert(id, (reply, method, now));
            }
            // The script is gone: the request is answered as no one held it.
            _ => send_back(
                core,
                reply,
                &method,
                http::not_found(scripts.flavour(), &id),
            ),
        }
    }
    for Returning {
        caller,
        request,
        answered,
        ..
    } in returning
    {
        let prim = prim_mates(scripts, caller);
        let _posted =
            scripts
                .engine
                .http_response(caller, &request, answered.status, &answered.body, &prim);
    }
}

/// After a tick: route what the region's scripts sent and answered, answer
/// the requests that waited too long, and release the URLs no script holds
/// any more.
pub(super) fn hand_out(core: &GridCore, index: usize, scripts: &mut RegionScripts) {
    let flavour = scripts.flavour();
    for (caller, request) in core::mem::take(&mut scripts.http.sent) {
        let reply = Reply::Script {
            region: index,
            caller,
            request: request.id.clone(),
        };
        if !arrive(
            core,
            reply,
            &request.method,
            &request.url,
            &request.headers,
            &request.body,
            LOOPBACK,
        ) {
            let responder = std::sync::Arc::clone(&core.script_http.lock().responder);
            let answered = responder.respond(&request);
            send_back(
                core,
                Reply::Script {
                    region: index,
                    caller,
                    request: request.id.clone(),
                },
                &request.method,
                answered,
            );
        }
    }
    for (request, answered) in core::mem::take(&mut scripts.http.answered) {
        if let Some((reply, method, _)) = scripts.http.waiting.remove(&request) {
            send_back(core, reply, &method, answered);
        }
    }
    let now = scripts.engine.now();
    let (wait, timed_out) = http::timeout(flavour);
    let ticks = wait
        .as_micros()
        .checked_div(scripts.engine.config().step.as_micros())
        .and_then(|ticks| u64::try_from(ticks).ok())
        .unwrap_or(u64::MAX);
    let late: Vec<String> = scripts
        .http
        .waiting
        .iter()
        .filter(|(_, (_, _, at))| now.0.saturating_sub(at.0) >= ticks)
        .map(|(key, _)| key.clone())
        .collect();
    for key in late {
        if let Some((reply, method, _)) = scripts.http.waiting.remove(&key) {
            send_back(core, reply, &method, timed_out.clone());
        }
    }
    let engine: &Engine = &scripts.engine;
    core.script_http.lock().urls.retain(|_, granted| {
        granted.region != index || engine.urls(granted.caller).contains(&granted.url)
    });
}

/// A request to `url` from `reply`'s sender, coming from `remote`: whether
/// `url` is one of the grid's script URLs. One that is reaches its script's
/// region at its next tick, or is answered by the URL surface; one whose
/// script has released it is answered as each grid answers that.
pub(crate) fn arrive(
    core: &GridCore,
    reply: Reply,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    remote: &str,
) -> bool {
    let own = format!("http://{}", authority(core));
    let Some(path) = url.strip_prefix(&own) else {
        return false;
    };
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let mut shared = core.script_http.lock();
    let found = [
        sl_lsl_runtime::vm::EngineFlavour::SecondLife,
        sl_lsl_runtime::vm::EngineFlavour::YEngine,
    ]
    .into_iter()
    .find_map(|flavour| {
        http::url_key(flavour, path).map(|(key, rest)| (flavour, key.to_owned(), rest.to_owned()))
    });
    let Some((flavour, key, rest)) = found else {
        return false;
    };
    let Some(granted) = shared.urls.get(&key).cloned() else {
        drop(shared);
        send_back(core, reply, method, http::not_found(flavour, &key));
        return true;
    };
    match http::arrive(
        granted.flavour,
        &granted.url,
        (method, &rest, query),
        remote,
        headers,
        body,
    ) {
        Arrival::Answered(answered) => {
            drop(shared);
            send_back(core, reply, method, answered);
        }
        Arrival::Script(inbound) => shared.arriving.push(Arriving {
            region: granted.region,
            caller: granted.caller,
            inbound,
            reply,
        }),
    }
    true
}

/// Send `answered` where `reply` says: a script's answer at its region's next
/// tick, or the HTTP service's waiting request. A `HEAD`'s answer carries no
/// body.
pub(crate) fn send_back(core: &GridCore, reply: Reply, method: &str, mut answered: Answered) {
    if method == "HEAD" {
        answered.body.clear();
    }
    match reply {
        Reply::Script {
            region,
            caller,
            request,
        } => core.script_http.lock().returning.push(Returning {
            region,
            caller,
            request,
            answered,
        }),
        Reply::Outside(sender) => {
            // Whoever asked may have given up; nothing is owed them then.
            let _gone = sender.send(answered);
        }
    }
}

/// The key a request arriving in region `index` is known by.
fn request_key(index: usize, arrival: u64) -> String {
    let region = u64::try_from(index).unwrap_or(u64::MAX) & 0xFFFF;
    format!(
        "4e4e4e4e-{region:04x}-4000-8000-{:012x}",
        arrival & 0xFFFF_FFFF_FFFF
    )
}

/// Every script in the prim `caller` is in.
fn prim_mates(scripts: &RegionScripts, caller: CallerId) -> Vec<CallerId> {
    scripts
        .slots
        .get(&caller)
        .map(|slot| scripts.scripts_in(slot.prim))
        .unwrap_or_default()
}
