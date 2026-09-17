//! Object-addressed capability requests (`GetObjectCost`,
//! `ResourceCostSelected`, `GetObjectPhysicsData`), each part sent to the
//! region that knows its objects — see [`sl_proto::NeighbourCaps`].

use std::time::Instant;

use crossbeam_channel::Sender;
use sl_proto::{
    CapRequest, CapRoute, Command, Llsd, NeighbourCaps, ObjectCapsCommand, SelectedCostKind,
    Session, build_get_object_cost_request, build_get_object_physics_data_request,
    build_resource_cost_selected_request, merge_selected_cost_replies,
};

use crate::caps::report_caps_failure;
use crate::voice::{post_cap_llsd, run_voice_cap};
use crate::{Caps, deliver};

/// Routes `command` — first routed at `since` — to the regions that know its
/// objects and spawns the POSTs, or parks it while a neighbour's capability
/// map is still on its way. Any other command is ignored.
///
/// A neighbour part that cannot be sent (its map failed, lacks the capability,
/// or never came) is logged and reported as a failed capability request, the
/// same way a failed POST is.
pub(crate) fn dispatch(
    command: &Command,
    since: Instant,
    session: &Session,
    caps: &Caps,
    neighbours: &mut NeighbourCaps,
    now: Instant,
) {
    let Some(request) = ObjectCapsCommand::of(command) else {
        return;
    };
    let requests = match neighbours.route(session, &caps.map, request, since, now) {
        CapRoute::Wait => {
            neighbours.park(command.clone(), since);
            return;
        }
        CapRoute::Ready {
            requests,
            unroutable,
        } => {
            for part in unroutable {
                tracing::warn!(
                    capability = request.capability,
                    sim = %part.sim,
                    objects = part.objects.len(),
                    "not asking a neighbour region about its objects: {}",
                    part.reason
                );
                report_caps_failure(&caps.events_tx, request.capability);
            }
            requests
        }
    };
    match command {
        Command::RequestObjectCost { .. } => {
            spawn_each(requests, request.capability, &caps.events_tx, |objects| {
                build_get_object_cost_request(objects)
            });
        }
        Command::RequestObjectPhysicsData { .. } => {
            spawn_each(requests, request.capability, &caps.events_tx, |objects| {
                build_get_object_physics_data_request(objects)
            });
        }
        Command::RequestSelectedCost { roots, .. } => {
            let kind = if *roots {
                SelectedCostKind::Roots
            } else {
                SelectedCostKind::Prims
            };
            spawn_selected_cost(requests, kind, request.capability, &caps.events_tx);
        }
        _ => {}
    }
}

/// Re-routes every parked request: those whose neighbour maps have arrived (or
/// whose wait ran out) are sent, the rest parked again.
fn dispatch_parked(session: &Session, caps: &Caps, neighbours: &mut NeighbourCaps, now: Instant) {
    for (command, since) in neighbours.take_parked() {
        dispatch(&command, since, session, caps, neighbours, now);
    }
}

/// One POST per region, each reply decoded on its own: the per-object replies
/// (`GetObjectCost`, `GetObjectPhysicsData`) name their objects, so the parts
/// need no joining.
fn spawn_each(
    requests: Vec<CapRequest>,
    capability: &'static str,
    events_tx: &Sender<(String, Llsd)>,
    body: impl Fn(&[sl_proto::ObjectKey]) -> String,
) {
    for part in requests {
        let body = body(&part.objects);
        let events_tx = events_tx.clone();
        std::thread::spawn(move || {
            run_voice_cap(&part.url, body, capability, &events_tx);
        });
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
    events_tx: &Sender<(String, Llsd)>,
) {
    if let [part] = requests.as_slice() {
        let body = build_resource_cost_selected_request(kind, &part.objects);
        let url = part.url.clone();
        let events_tx = events_tx.clone();
        std::thread::spawn(move || run_voice_cap(&url, body, capability, &events_tx));
        return;
    }
    if requests.is_empty() {
        return;
    }
    let events_tx = events_tx.clone();
    std::thread::spawn(move || {
        let replies: Option<Vec<Llsd>> = requests
            .iter()
            .map(|part| {
                post_cap_llsd(
                    &part.url,
                    build_resource_cost_selected_request(kind, &part.objects),
                )
            })
            .collect();
        let merged = replies.and_then(|replies| match merge_selected_cost_replies(&replies) {
            Ok(merged) => Some(merged),
            Err(error) => {
                tracing::warn!(
                    capability,
                    "a ResourceCostSelected reply did not decode: {error}"
                );
                None
            }
        });
        match merged {
            Some(merged) => deliver(&events_tx, (capability.to_owned(), merged)),
            None => report_caps_failure(&events_tx, capability),
        }
    });
}

/// Records a neighbour's capability map arriving and sends whatever was
/// waiting for it; forgets the maps of neighbours that are gone.
pub(crate) fn drain_neighbour_maps(
    session: &Session,
    caps: &Caps,
    neighbours: &mut NeighbourCaps,
    now: Instant,
) {
    while let Ok((sim, outcome)) = caps.neighbour_map_rx.try_recv() {
        if let Err(reason) = &outcome {
            tracing::warn!(%sim, "a neighbour region's capabilities could not be fetched: {reason}");
        }
        neighbours.fetched(sim, outcome);
    }
    neighbours.prune(session);
    if neighbours.has_parked() {
        dispatch_parked(session, caps, neighbours, now);
    }
}
