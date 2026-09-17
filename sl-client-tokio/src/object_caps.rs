//! Object-addressed capability requests (`GetObjectCost`,
//! `ResourceCostSelected`, `GetObjectPhysicsData`), each part sent to the
//! region that knows its objects — see [`sl_proto::NeighbourCaps`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use reqwest::Client as ReqwestClient;
use sl_proto::{
    CapRequest, CapRoute, Command, Llsd, NeighbourCaps, ObjectCapsCommand, ObjectKey,
    SelectedCostKind, Session, build_get_object_cost_request,
    build_get_object_physics_data_request, build_resource_cost_selected_request,
    merge_selected_cost_replies,
};
use tokio::sync::mpsc;

use crate::caps::{MAX_SEED_FETCH_RETRIES, deliver, fetch_capabilities, report_caps_failure};
use crate::retry::transient_backoff;
use crate::voice::{post_cap_llsd, post_voice_cap};

/// A neighbouring region's capability map, or why it could not be fetched,
/// keyed by the neighbour's simulator address.
pub(crate) type NeighbourMapOutcome = (SocketAddr, Result<HashMap<String, String>, String>);

/// POSTs a neighbour region's seed capability, so the simulator marks the
/// agent's capabilities as sent and begins streaming that region's scene to
/// the child circuit, and reports the neighbour's capability map (or why it
/// could not be had) over `map_tx` — the map object-addressed capability
/// requests about the neighbour's objects are sent to. A failed fetch is
/// retried like the root region's.
pub(crate) async fn fetch_neighbour_caps(
    sim: SocketAddr,
    seed: url::Url,
    http: ReqwestClient,
    map_tx: mpsc::Sender<NeighbourMapOutcome>,
) {
    let mut outcome = fetch_capabilities(Some(&seed), &http).await;
    for attempt in 0..MAX_SEED_FETCH_RETRIES {
        let Err(error) = &outcome else {
            break;
        };
        tracing::warn!(%sim, %seed, attempt, %error, "neighbour seed-capabilities fetch failed");
        tokio::time::sleep(transient_backoff(attempt)).await;
        outcome = fetch_capabilities(Some(&seed), &http).await;
    }
    deliver(&map_tx, (sim, outcome.map_err(|error| error.to_string()))).await;
}

/// Routes `command` — first routed at `since` — to the regions that know its
/// objects and spawns the POSTs, or parks it while a neighbour's capability
/// map is still on its way. Any other command is ignored.
///
/// A neighbour part that cannot be sent (its map failed, lacks the capability,
/// or never came) is logged and reported as a failed capability request, the
/// same way a failed POST is.
pub(crate) async fn dispatch(
    command: Command,
    since: Instant,
    session: &Session,
    root_caps: &HashMap<String, String>,
    neighbours: &mut NeighbourCaps,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) {
    let Some(request) = ObjectCapsCommand::of(&command) else {
        return;
    };
    let capability = request.capability;
    let requests = match neighbours.route(session, root_caps, request, since, Instant::now()) {
        CapRoute::Wait => {
            neighbours.park(command, since);
            return;
        }
        CapRoute::Ready {
            requests,
            unroutable,
        } => {
            for part in unroutable {
                tracing::warn!(
                    capability,
                    sim = %part.sim,
                    objects = part.objects.len(),
                    "not asking a neighbour region about its objects: {}",
                    part.reason
                );
                report_caps_failure(caps_tx, capability).await;
            }
            requests
        }
    };
    match command {
        Command::RequestObjectCost { .. } => {
            spawn_each(
                requests,
                capability,
                http,
                caps_tx,
                build_get_object_cost_request,
            );
        }
        Command::RequestObjectPhysicsData { .. } => {
            spawn_each(
                requests,
                capability,
                http,
                caps_tx,
                build_get_object_physics_data_request,
            );
        }
        Command::RequestSelectedCost { roots, .. } => {
            let kind = if roots {
                SelectedCostKind::Roots
            } else {
                SelectedCostKind::Prims
            };
            spawn_selected_cost(requests, kind, capability, http, caps_tx);
        }
        _ => {}
    }
}

/// Records neighbour capability maps, forgets those of neighbours that are
/// gone, and re-routes every parked request: those whose maps have arrived (or
/// whose wait ran out) are sent, the rest parked again.
pub(crate) async fn dispatch_parked(
    session: &Session,
    root_caps: &HashMap<String, String>,
    neighbours: &mut NeighbourCaps,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) {
    neighbours.prune(session);
    for (command, since) in neighbours.take_parked() {
        dispatch(
            command, since, session, root_caps, neighbours, http, caps_tx,
        )
        .await;
    }
}

/// One POST per region, each reply decoded on its own: the per-object replies
/// (`GetObjectCost`, `GetObjectPhysicsData`) name their objects, so the parts
/// need no joining.
fn spawn_each(
    requests: Vec<CapRequest>,
    capability: &'static str,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
    body: fn(&[ObjectKey]) -> String,
) {
    for part in requests {
        tokio::spawn(post_voice_cap(
            part.url,
            body(&part.objects),
            capability,
            http.clone(),
            caps_tx.clone(),
        ));
    }
}

/// The summed costs of a selection: one region's reply is the answer; a
/// selection spanning regions is asked of each and the replies added, so the
/// session sees the one reply the whole selection would have had. If any part
/// fails, the request is reported failed rather than answered with part of the
/// selection.
fn spawn_selected_cost(
    requests: Vec<CapRequest>,
    kind: SelectedCostKind,
    capability: &'static str,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) {
    match requests.as_slice() {
        [] => {}
        [part] => {
            tokio::spawn(post_voice_cap(
                part.url.clone(),
                build_resource_cost_selected_request(kind, &part.objects),
                capability,
                http.clone(),
                caps_tx.clone(),
            ));
        }
        _ => {
            let http = http.clone();
            let caps_tx = caps_tx.clone();
            tokio::spawn(async move {
                let mut replies = Vec::with_capacity(requests.len());
                for part in &requests {
                    let body = build_resource_cost_selected_request(kind, &part.objects);
                    let Some(reply) = post_cap_llsd(&part.url, body, &http).await else {
                        report_caps_failure(&caps_tx, capability).await;
                        return;
                    };
                    replies.push(reply);
                }
                match merge_selected_cost_replies(&replies) {
                    Ok(merged) => deliver(&caps_tx, (capability.to_owned(), merged)).await,
                    Err(error) => {
                        tracing::warn!(
                            capability,
                            "a ResourceCostSelected reply did not decode: {error}"
                        );
                        report_caps_failure(&caps_tx, capability).await;
                    }
                }
            });
        }
    }
}
