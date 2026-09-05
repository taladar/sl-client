//! The parcel and region half of a client's edits: the About Land floater, the
//! land it buys and abandons, and the region's own configuration.
//!
//! The sibling of [`crate::object_edits`], and the same shape: a message
//! [`SimSession`] decodes, a record in the region it changes, and a push that
//! makes the change readable back. What differs is the push. An object has two
//! records travelling in two messages; a parcel has **one**, and a simulator
//! re-sends the whole of it — a `ParcelProperties` with sequence id zero, the
//! unsolicited form the arrival burst already uses. That single record is also
//! what makes a parcel edit dangerous in a way an object edit is not: the About
//! Land form carries every field back, so a floater populated from a stale read
//! reverts whatever somebody else changed in the meantime. Reproducing that is
//! [`test-fake-grid-concurrent-edits`]'s job; storing the write is this
//! module's.
//!
//! The **access lists** are the one parcel record that does not travel in the
//! properties reply. They have their own request and their own reply, and they
//! live here beside the parcels rather than on them, because a `ParcelInfo` is
//! the wire record and has no field for them.
//!
//! [`test-fake-grid-concurrent-edits`]: https://example.invalid/roadmap

use std::time::Instant;

use sl_proto::{
    ParcelInfo, ParcelStatus, RegionIdentity, RegionLocalParcelId, ServerEvent, SimSession,
};
use sl_types::key::{AgentKey, OwnerKey};
use sl_types::money::LindenAmount;

use crate::world::{RegionChange, SceneFixtures, region_limits};

/// The sequence id of an unsolicited parcel push — what a simulator re-sends a
/// changed parcel under, and what the arrival burst already uses.
const UNSOLICITED_SEQUENCE_ID: i32 = 0;

/// Answers one drained [`ServerEvent`] that edits a parcel or asks about the
/// region, returning the [`RegionChange`]s the region's other sessions have to
/// be told about — or [`None`] when the event is neither, which is how
/// [`answer_world_request`](crate::world::answer_world_request) knows to carry
/// on looking.
pub(crate) fn answer_parcel_edit(
    world: &mut SceneFixtures,
    identity: &RegionIdentity,
    agent_id: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    match event {
        // The whole About Land form. Every field is re-asserted, so the record
        // is rewritten rather than patched — which is what a real simulator
        // does with it, and the reason a stale floater can revert a change it
        // never saw.
        ServerEvent::ParcelPropertiesUpdated { update } => {
            let Some(parcel) = world.parcel_mut(update.local_id) else {
                tracing::debug!(
                    "an About Land save named parcel {:?}, which is not here",
                    update.local_id
                );
                return Some(Vec::new());
            };
            parcel.raw_parcel_flags = update.parcel_flags.bits();
            parcel.sale_price.clone_from(&update.sale_price);
            parcel.name.clone_from(&update.name);
            parcel.description.clone_from(&update.description);
            parcel.music_url.clone_from(&update.music_url);
            parcel.media_url.clone_from(&update.media_url);
            parcel.media_id = update.media_id;
            parcel.media_auto_scale = update.media_auto_scale;
            parcel.group = update.group_id;
            parcel.pass_price.clone_from(&update.pass_price);
            parcel.pass_hours = update.pass_hours;
            parcel.category = update.category;
            parcel.auth_buyer_id = update.auth_buyer_id;
            parcel.snapshot_id = update.snapshot_id;
            parcel.user_location = update.user_location;
            parcel.user_look_at = update.user_look_at;
            parcel.landing_type = sl_proto::LandingType::from_u8(update.landing_type);
            push_parcel(world, update.local_id, sim, now);
        }
        // A purchase: the buyer owns it and it comes off the market. The fake
        // grid charges nobody — its economy is a price list, not a ledger — so
        // the price the client believes it is paying is noted and dropped.
        ServerEvent::ParcelBought {
            local_id,
            group_id,
            is_group_owned,
            ..
        } => {
            let owner = match (is_group_owned, group_id) {
                (true, Some(group)) => OwnerKey::Group(*group),
                _ => OwnerKey::Agent(agent_id),
            };
            set_owner(world, *local_id, owner, ParcelStatus::Leased);
            push_parcel(world, *local_id, sim, now);
        }
        ServerEvent::ParcelDeededToGroup { local_id, group_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Group(*group_id),
                ParcelStatus::Leased,
            );
            push_parcel(world, *local_id, sim, now);
        }
        // Abandoning hands the land back to the estate: the region's owner
        // holds it, and its status says nobody chose to.
        ServerEvent::ParcelReleased { local_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Agent(AgentKey::from(identity.sim_owner)),
                ParcelStatus::Abandoned,
            );
            push_parcel(world, *local_id, sim, now);
        }
        // Reclaiming is the estate manager taking abandoned land back into use.
        ServerEvent::ParcelReclaimed { local_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Agent(AgentKey::from(identity.sim_owner)),
                ParcelStatus::Leased,
            );
            push_parcel(world, *local_id, sim, now);
        }
        // A return takes the objects out of the world. A real grid also files
        // each one into its owner's Lost and Found; the fake grid has one
        // agent's inventory to file into and no owner but that agent, so what
        // is observable — and what is done here — is the removal.
        ServerEvent::ParcelObjectsReturned {
            local_id,
            task_ids,
            owner_ids,
            ..
        } => {
            let doomed: Vec<sl_proto::RegionLocalObjectId> = world
                .objects
                .iter()
                .filter(|object| {
                    world.parcel_at_position(&object.motion.position) == Some(*local_id)
                })
                .filter(|object| {
                    task_ids.contains(&object.full_id)
                        || owner_ids
                            .iter()
                            .any(|owner| owner.uuid() == object.owner_id)
                })
                .map(|object| object.local_id)
                .collect();
            let mut changes = Vec::new();
            for local_id in &doomed {
                if world.remove_object(*local_id).is_some() {
                    changes.push(RegionChange::Killed(*local_id));
                }
            }
            if !doomed.is_empty()
                && let Err(error) = sim.send_kill_object(&doomed, now)
            {
                tracing::warn!("killing a returned object failed: {error}");
            }
            return Some(changes);
        }
        // "Show me what I would be returning": the simulator highlights the
        // objects in the viewer rather than changing anything.
        ServerEvent::ParcelObjectsSelected {
            local_id,
            owner_ids,
            ..
        } => {
            let highlighted: Vec<sl_proto::RegionLocalObjectId> = world
                .objects
                .iter()
                .filter(|object| {
                    world.parcel_at_position(&object.motion.position) == Some(*local_id)
                })
                .filter(|object| {
                    owner_ids.is_empty()
                        || owner_ids
                            .iter()
                            .any(|owner| owner.uuid() == object.owner_id)
                })
                .map(|object| object.local_id)
                .collect();
            if let Err(error) = sim.send_force_object_select(true, &highlighted, now) {
                tracing::warn!("highlighting a parcel's objects failed: {error}");
            }
        }
        ServerEvent::RequestParcelAccessList {
            local_id,
            scope,
            sequence_id,
        } => {
            let entries = world.access_list(*local_id, *scope).to_vec();
            if let Err(error) =
                sim.send_parcel_access_list_reply(*local_id, *scope, *sequence_id, &entries, now)
            {
                tracing::warn!("answering a parcel access list request failed: {error}");
            }
        }
        // A list arrives in sections, and a section replaces what it covers:
        // the first one replaces the list, the rest append to it. That is what
        // makes a two-section update land as one list rather than as its last
        // section alone.
        ServerEvent::ParcelAccessListUpdated {
            local_id,
            scope,
            entries,
            sequence_id,
            ..
        } => {
            let held = world.access_list_mut(*local_id, *scope);
            if *sequence_id <= 1 {
                held.clear();
            }
            held.extend(entries.iter().copied());
        }
        // The top-scripts / top-colliders report. A fake region runs no
        // scripts and simulates no physics, so the honest answer is an empty
        // report rather than no answer at all — a viewer that gets nothing
        // waits out its own timeout and shows the same empty list.
        ServerEvent::RequestLandStat {
            report_type,
            request_flags,
            ..
        } => {
            if let Err(error) = sim.send_land_stat_reply(*report_type, *request_flags, 0, &[], now)
            {
                tracing::warn!("answering a land stat request failed: {error}");
            }
        }
        ServerEvent::RequestRegionInfo => {
            if let Err(error) = sim.send_region_info(&region_limits(identity), now) {
                tracing::warn!("answering a region info request failed: {error}");
            }
        }
        _other => return None,
    }
    Some(Vec::new())
}

/// Sets a parcel's owner and ownership status, and takes it off the market:
/// every path that changes who holds land is also the end of its sale.
fn set_owner(
    world: &mut SceneFixtures,
    local_id: RegionLocalParcelId,
    owner: OwnerKey,
    status: ParcelStatus,
) {
    let Some(parcel) = world.parcel_mut(local_id) else {
        tracing::debug!("a land transfer named parcel {local_id:?}, which is not here");
        return;
    };
    parcel.owner = owner;
    parcel.status = status;
    parcel.sale_price = None;
    parcel.raw_parcel_flags = sl_wire::ParcelFlags::from_bits(parcel.raw_parcel_flags)
        .difference(sl_wire::ParcelFlags::FOR_SALE)
        .bits();
    parcel.claim_price = LindenAmount(0);
    parcel.auth_buyer_id = None;
}

/// Re-sends a changed parcel's whole record to the editing client, under the
/// sequence id of an unsolicited push — the only message a parcel's fields
/// travel in.
fn push_parcel(
    world: &SceneFixtures,
    local_id: RegionLocalParcelId,
    sim: &mut SimSession,
    now: Instant,
) {
    let Some(parcel) = world.parcel_by_local_id(local_id) else {
        return;
    };
    let mut record: ParcelInfo = parcel.clone();
    record.sequence_id = UNSOLICITED_SEQUENCE_ID;
    if let Err(error) = sim.send_parcel_properties(&record, now) {
        tracing::warn!("re-sending an edited parcel failed: {error}");
    }
}
