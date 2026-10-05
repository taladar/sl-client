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
//! reverts whatever somebody else changed in the meantime. Nothing here stops
//! that, and nothing on a real grid does either — a simulator cannot tell a
//! re-asserted field from an unchanged one. What it can do is make the revert
//! *avoidable*, by re-sending the whole record to the parcel's other occupants
//! ([`RegionChange::ParcelChanged`]) so a floater that re-seeds from the push
//! carries the other resident's change forward instead of undoing it.
//!
//! The **access lists** are the one parcel record that does not travel in the
//! properties reply. They have their own request and their own reply, and they
//! live here beside the parcels rather than on them, because a `ParcelInfo` is
//! the wire record and has no field for them.

use std::time::Instant;

use sl_proto::{
    LandStatExtended, LandStatItem, LandStatReportType, ParcelInfo, ParcelObjectOwner,
    ParcelReturnType, ParcelStatus, ParcelUpdate, RegionIdentity, RegionLocalParcelId, ServerEvent,
    SimSession, pcode,
};
use sl_types::key::{AgentKey, OwnerKey};
use sl_types::map::RegionCoordinates;
use sl_types::money::LindenAmount;

use crate::imitates::{EditEcho, ParcelPolicy};
use crate::world::{AvatarIdentity, RegionChange, SceneFixtures, region_limits};

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
    (region, agent): (&RegionIdentity, &AvatarIdentity),
    policy: ParcelPolicy,
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
            apply_update(parcel, update, policy);
            let echo = match policy.edit_echo {
                EditEcho::Fixed(sequence_id) => sequence_id,
                EditEcho::LastSequence => parcel.sequence_id,
            };
            return Some(echo_parcel(world, update.local_id, echo, sim));
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
                _ => OwnerKey::Agent(agent.agent_id),
            };
            set_owner(world, *local_id, owner, ParcelStatus::Leased);
            return Some(push_parcel(world, *local_id, sim));
        }
        ServerEvent::ParcelDeededToGroup { local_id, group_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Group(*group_id),
                ParcelStatus::Leased,
            );
            return Some(push_parcel(world, *local_id, sim));
        }
        // Abandoning hands the land back to the estate: the region's owner
        // holds it, and its status says nobody chose to.
        ServerEvent::ParcelReleased { local_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Agent(AgentKey::from(region.sim_owner)),
                ParcelStatus::Abandoned,
            );
            return Some(push_parcel(world, *local_id, sim));
        }
        // Reclaiming is the estate manager taking abandoned land back into use.
        ServerEvent::ParcelReclaimed { local_id } => {
            set_owner(
                world,
                *local_id,
                OwnerKey::Agent(AgentKey::from(region.sim_owner)),
                ParcelStatus::Leased,
            );
            return Some(push_parcel(world, *local_id, sim));
        }
        // A return takes the objects out of the world. A real grid also files
        // each one into its owner's Lost and Found; the fake grid has one
        // agent's inventory to file into and no owner but that agent, so what
        // is observable — and what is done here — is the removal.
        ServerEvent::ParcelObjectsReturned {
            local_id,
            return_type,
            task_ids,
            owner_ids,
        } => {
            let request = ReturnRequest {
                local_id: *local_id,
                return_type: *return_type,
                task_ids,
                owner_ids,
            };
            let doomed: Vec<sl_proto::RegionLocalObjectId> = world
                .objects
                .iter()
                .filter(|object| on_scope(world, object, *local_id))
                .filter(|object| returned(world, object, &request, policy))
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
        // Stopping the named objects' scripts. A fake region runs none, so the
        // whole of what a real grid does here is unobservable — but the request
        // is a real one (the top-objects window's Disable), and an unanswered
        // event is indistinguishable from a grid that does not understand it.
        // Accepting it and leaving the objects where they are is the honest
        // answer, and says so.
        ServerEvent::DisableParcelObjects {
            local_id,
            task_ids,
            owner_ids,
            ..
        } => {
            let stilled = world
                .objects
                .iter()
                .filter(|object| on_scope(world, object, *local_id))
                .filter(|object| {
                    task_ids.contains(&object.full_id) || owner_ids.contains(&object.owner_id)
                })
                .count();
            tracing::debug!(
                "a client disabled the scripts of {stilled} object(s); this region runs none"
            );
            return Some(Vec::new());
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
        // The top-scripts / top-colliders report, built from what the scene
        // says its objects cost ([`ObjectCost`]). A fake region runs no scripts
        // and simulates no physics, so a report can only be a scene's own
        // statement — and a scene that states nothing is answered with an empty
        // report rather than with no answer at all, since a viewer that gets
        // nothing waits out its own timeout and shows the same empty list.
        //
        // Over the **event queue**, because that is where a region with one
        // answers this (OpenSim's `SendLandStatReply` only falls back to the UDP
        // packet when it has no queue, and the message is `UDPDeprecated` for
        // the same reason). A grid that answered by packet would let a viewer
        // that only understands the packet pass here and fail on every real
        // grid.
        ServerEvent::RequestLandStat {
            report_type,
            request_flags,
            filter,
            local_id,
        } => {
            let rows = land_stat_rows(
                world,
                agent,
                *report_type,
                *request_flags,
                filter,
                *local_id,
            );
            let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
            sim.enqueue_land_stat_reply(*report_type, *request_flags, total, &rows);
        }
        // A parcel's object-owner tally, from the scene: one row per owner,
        // counting the prims of every linkset whose root stands on the parcel
        // (a simulator's `primsOverMe`, tallied by `PrimCount`). Over the event
        // queue where the imitated grid uses it — the message is
        // `UDPDeprecated`, and only the queue's form is whole in one document
        // — and as the UDP message on OpenSim, which still sends that.
        ServerEvent::RequestParcelObjectOwners { local_id } => {
            let owners = parcel_object_owners(world, *local_id);
            if policy.owners_reply_over_event_queue {
                sim.enqueue_parcel_object_owners_reply(&owners);
            } else if let Err(error) = sim.send_parcel_object_owners_reply(&owners, now) {
                tracing::warn!("answering an object-owners request failed: {error}");
            }
        }
        ServerEvent::RequestRegionInfo => {
            if let Err(error) = sim.send_region_info(&region_limits(region), now) {
                tracing::warn!("answering a region info request failed: {error}");
            }
        }
        _other => return None,
    }
    Some(Vec::new())
}

/// The object-owner tally of one parcel: each owner's prim count over the
/// linksets whose root stands on it, in the order the owners are first met.
///
/// Avatars are not objects anybody owns on a parcel, and an attachment's root
/// is its wearer — neither is counted. A fake region keeps no rez times, so no
/// row says when its owner last rezzed.
fn parcel_object_owners(
    world: &SceneFixtures,
    parcel: RegionLocalParcelId,
) -> Vec<ParcelObjectOwner> {
    let objects = world.all_objects();
    let mut tally: Vec<ParcelObjectOwner> = Vec::new();
    for root in objects
        .iter()
        .filter(|object| object.parent_id.0 == 0 && object.pcode != pcode::AVATAR)
    {
        if world.parcel_at_position(&root.motion.position) != Some(parcel) {
            continue;
        }
        let children = objects
            .iter()
            .filter(|child| child.parent_id == root.local_id && child.pcode != pcode::AVATAR)
            .count();
        let prims = i32::try_from(children.saturating_add(1)).unwrap_or(i32::MAX);
        let owner = world.properties_of(root.local_id).map_or_else(
            || OwnerKey::Agent(AgentKey::from(root.owner_id)),
            |properties| properties.owner,
        );
        match tally.iter_mut().find(|row| row.owner == owner) {
            Some(row) => row.count = row.count.saturating_add(prims),
            None => tally.push(ParcelObjectOwner {
                owner,
                count: prims,
                online_status: false,
                most_recent: None,
            }),
        }
    }
    tally
}

/// A `ParcelReturnObjects` as the client sent it.
struct ReturnRequest<'a> {
    /// The parcel, or [`WHOLE_REGION`].
    local_id: RegionLocalParcelId,
    /// The classes of object to return (`RT_*`, a bitfield).
    return_type: ParcelReturnType,
    /// The objects named one by one.
    task_ids: &'a [sl_types::key::ObjectKey],
    /// The owners named for [`ParcelReturnType::LIST`].
    owner_ids: &'a [OwnerKey],
}

/// Whether a return takes `object`, which is already in scope.
///
/// Addressed to the whole region it takes exactly the named objects — the
/// top-objects window's return, which both grids honour. Addressed to a parcel
/// it takes the classes the return type names, measured against the parcel's
/// owner and group: `RT_OWNER` the parcel owner's objects, `RT_GROUP` the
/// group's other than the owner's, `RT_OTHER` everybody else's, `RT_LIST` the
/// named owners'. OpenSim stops there (`LandObject.ReturnLandObjects` never
/// reads the task list), so on its flavour naming one object on a parcel
/// returns nothing ([`ParcelPolicy::parcel_return_reads_task_ids`]).
fn returned(
    world: &SceneFixtures,
    object: &sl_proto::Object,
    request: &ReturnRequest<'_>,
    policy: ParcelPolicy,
) -> bool {
    let named = request.task_ids.contains(&object.full_id);
    if request.local_id == WHOLE_REGION {
        return named;
    }
    if named && policy.parcel_return_reads_task_ids {
        return true;
    }
    let Some(parcel) = world.parcel_by_local_id(request.local_id) else {
        return false;
    };
    let properties = world.properties_of(object.local_id);
    let owner = properties.as_ref().map_or_else(
        || OwnerKey::Agent(AgentKey::from(object.owner_id)),
        |properties| properties.owner,
    );
    let group = properties.and_then(|properties| properties.group);
    let land_owners = owner == parcel.owner;
    let land_group = parcel.group.is_some() && group == parcel.group;
    let wants = |class: ParcelReturnType| request.return_type.0 & class.0 != 0;
    (wants(ParcelReturnType::OWNER) && land_owners)
        || (wants(ParcelReturnType::GROUP) && land_group && !land_owners)
        || (wants(ParcelReturnType::OTHER) && !land_owners && !land_group)
        || (wants(ParcelReturnType::LIST)
            && request
                .owner_ids
                .iter()
                .any(|named| named.uuid() == owner.uuid()))
}

/// Whether an object is in scope for a return or a disable addressed to
/// `local_id`.
///
/// A parcel's id means "standing on that parcel". **`-1` means the whole
/// region**, and it is how the top-objects return names objects wherever they
/// stand — the reference sends `LocalID = -1` with `RT_NONE` and an explicit
/// task-id list, and OpenSim branches on exactly that
/// (`LandManagementModule::ReturnObjectsInParcel`). A grid that only matched by
/// parcel would accept that request and silently do nothing.
fn on_scope(
    world: &SceneFixtures,
    object: &sl_proto::Object,
    local_id: RegionLocalParcelId,
) -> bool {
    local_id == WHOLE_REGION || world.parcel_at_position(&object.motion.position) == Some(local_id)
}

/// The whole region, as the scope of a return or a disable.
const WHOLE_REGION: RegionLocalParcelId = RegionLocalParcelId(-1);

/// The most rows one report carries — OpenSim's own cap, and the reason a
/// report is "top objects" rather than "every object".
const MAX_REPORT_ROWS: usize = 100;

/// The `RequestFlags` bit that scopes a report to one parcel
/// (`STAT_FILTER_BY_PARCEL`).
const FILTER_BY_PARCEL: u32 = 0x0000_0001;

/// The `RequestFlags` bit that narrows a report to an owner name
/// (`STAT_FILTER_BY_OWNER`).
const FILTER_BY_OWNER: u32 = 0x0000_0002;

/// The `RequestFlags` bit that narrows a report to an object name
/// (`STAT_FILTER_BY_OBJECT`).
const FILTER_BY_OBJECT: u32 = 0x0000_0004;

/// The `RequestFlags` bit that narrows a report to a parcel name
/// (`STAT_FILTER_BY_PARCEL_NAME`).
const FILTER_BY_PARCEL_NAME: u32 = 0x0000_0008;

/// The rows of a top-objects report: every object the scene says costs
/// something in the unit this report asks for, narrowed by the request's scope
/// and filter, highest first, capped at [`MAX_REPORT_ROWS`].
///
/// The filter is the region's work, not the viewer's — the viewer sends a string
/// and a flag saying what it applies to, and gets back a report already
/// narrowed. Matching is a case-insensitive `contains`, as OpenSim's own
/// handler does it.
fn land_stat_rows(
    world: &SceneFixtures,
    agent: &AvatarIdentity,
    report_type: LandStatReportType,
    request_flags: u32,
    filter: &str,
    parcel_local_id: RegionLocalParcelId,
) -> Vec<LandStatItem> {
    let needle = filter.trim().to_lowercase();
    let has_filter = !needle.is_empty() && request_flags & FILTER_BY_ANY_NAME != 0;
    let mut rows: Vec<LandStatItem> = world
        .all_objects()
        .into_iter()
        .filter_map(|object| {
            let cost = world.object_costs.get(&object.local_id)?;
            let score = cost.score_for(report_type)?;
            let parcel = world.parcel_at_position(&object.motion.position);
            // `ParcelLocalID` scopes the report; the reference sends `0` for
            // the whole region, and OpenSim only honours the scope when the
            // by-parcel flag is set.
            if (parcel_local_id.0 != 0 || request_flags & FILTER_BY_PARCEL != 0)
                && parcel != Some(parcel_local_id)
            {
                return None;
            }
            let properties = world.properties_of(object.local_id);
            let task_name = properties
                .as_ref()
                .map_or_else(String::new, |properties| properties.name.clone());
            let owner_name = owner_name_of(world, agent, object.owner_id);
            let parcel_name = parcel
                .and_then(|local_id| world.parcel_by_local_id(local_id))
                .map_or_else(String::new, |parcel| parcel.name.clone());
            if has_filter {
                let subject = if request_flags & FILTER_BY_OWNER != 0 {
                    &owner_name
                } else if request_flags & FILTER_BY_OBJECT != 0 {
                    &task_name
                } else {
                    &parcel_name
                };
                if !subject.to_lowercase().contains(&needle) {
                    return None;
                }
            }
            Some(LandStatItem {
                task_local_id: object.local_id,
                task_id: object.full_id,
                location: RegionCoordinates::new(
                    object.motion.position.x,
                    object.motion.position.y,
                    object.motion.position.z,
                ),
                score,
                task_name,
                owner_name,
                // The event-queue form of the reply carries this half, and the
                // viewer has four columns for it.
                extended: Some(LandStatExtended {
                    mono_score: 0.0,
                    owner_id: Some(AgentKey::from(object.owner_id)),
                    parcel_name,
                    public_urls: cost.public_urls,
                    script_size_bytes: cost.script_memory_bytes,
                    timestamp: properties.map_or(0, |properties| {
                        u32::try_from(properties.creation_date).unwrap_or(u32::MAX)
                    }),
                }),
            })
        })
        .collect();
    // Highest first, as the report is defined: "top" objects.
    rows.sort_by(|left, right| {
        right
            .score
            .raw()
            .partial_cmp(&left.score.raw())
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    rows.truncate(MAX_REPORT_ROWS);
    rows
}

/// Any of the three name-filter bits.
const FILTER_BY_ANY_NAME: u32 = FILTER_BY_OWNER | FILTER_BY_OBJECT | FILTER_BY_PARCEL_NAME;

/// The legacy name of an object's owner. The arriving agent is the only account
/// a fake grid knows by name; anything else is named by the scene's NPCs, and an
/// owner that is neither reads as the empty string a report carries for an owner
/// the region cannot resolve.
fn owner_name_of(world: &SceneFixtures, agent: &AvatarIdentity, owner_id: uuid::Uuid) -> String {
    if owner_id == agent.agent_id.uuid() {
        return format!("{} {}", agent.first_name, agent.last_name);
    }
    world
        .npcs
        .iter()
        .find(|npc| npc.identity.agent_id.uuid() == owner_id)
        .map_or_else(String::new, |npc| {
            format!("{} {}", npc.identity.first_name, npc.identity.last_name)
        })
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

/// Writes an About Land save into the stored record. Every field the form
/// carries is re-asserted, so the record is rewritten rather than patched.
fn apply_update(parcel: &mut ParcelInfo, update: &ParcelUpdate, policy: ParcelPolicy) {
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
    // What only the capability form carries: taken when sent. An edit
    // over UDP has no media block, and the record keeps its own —
    // except where the imitated grid is OpenSim, which nulls the type
    // ([`ParcelPolicy::udp_edit_nulls_media_type`]).
    match (&update.media_data, &mut parcel.media_data) {
        (Some(media), stored) => *stored = Some(media.clone()),
        (None, Some(stored)) if policy.udp_edit_nulls_media_type => {
            stored.media_type.clear();
        }
        (None, _) => {}
    }
    if let Some(sharing) = &update.media_sharing {
        parcel.media_sharing = Some(sharing.clone());
    }
    parcel.see_avs = update.see_avs.or(parcel.see_avs);
    parcel.any_av_sounds = update.any_av_sounds.or(parcel.any_av_sounds);
    parcel.group_av_sounds = update.group_av_sounds.or(parcel.group_av_sounds);
    parcel.obscure_moap = update.obscure_moap.or(parcel.obscure_moap);
    // What the grid does not have, it does not keep.
    policy.dress(parcel);
}

/// Pushes the edited parcel back to the editing agent under `sequence_id` —
/// the echo the update's `flags` ask for, under the id the imitated grid uses
/// ([`EditEcho`]) — and returns the record under the sequence id of an
/// unsolicited push as the change the parcel's other occupants have to be told
/// about.
fn echo_parcel(
    world: &SceneFixtures,
    local_id: RegionLocalParcelId,
    sequence_id: i32,
    sim: &mut SimSession,
) -> Vec<RegionChange> {
    let Some(parcel) = world.parcel_by_local_id(local_id) else {
        return Vec::new();
    };
    let mut echo: ParcelInfo = parcel.clone();
    echo.sequence_id = sequence_id;
    // Over the event queue, as both live grids deliver a parcel record; the
    // UDP message has no room for the media and extended-flag blocks.
    sim.enqueue_parcel_properties(&echo);
    let mut record = echo;
    record.sequence_id = UNSOLICITED_SEQUENCE_ID;
    vec![RegionChange::ParcelChanged(Box::new(record))]
}

/// Re-sends a changed parcel's whole record to the editing client, under the
/// sequence id of an unsolicited push — the only message a parcel's fields
/// travel in — and returns the same record as the change the parcel's other
/// occupants have to be told about.
fn push_parcel(
    world: &SceneFixtures,
    local_id: RegionLocalParcelId,
    sim: &mut SimSession,
) -> Vec<RegionChange> {
    echo_parcel(world, local_id, UNSOLICITED_SEQUENCE_ID, sim)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::{ParcelMediaData, ParcelReturnType, ParcelUpdate, RegionLocalParcelId};
    use sl_types::key::{AgentKey, ObjectKey, OwnerKey};

    use super::{ReturnRequest, WHOLE_REGION, apply_update, returned};
    use crate::imitates::ImitatedGrid;
    use crate::world::{SceneFixtures, box_prim, region_wide_parcel};

    /// A parcel with media set, as both grids store it.
    fn parcel_with_media() -> sl_proto::ParcelInfo {
        let mut parcel = region_wide_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            "Land",
        );
        parcel.media_data = Some(ParcelMediaData {
            media_type: "video/mp4".to_owned(),
            ..ParcelMediaData::default()
        });
        parcel
    }

    /// The teeth of the OpenSim flavour: an edit over UDP, which has no media
    /// block, empties the media type there — so a client that falls back to
    /// UDP where the capability is granted loses it, as it does live — and
    /// leaves it alone on the Second Life flavour.
    #[test]
    fn a_udp_edit_wipes_the_media_type_only_on_opensim() {
        for (imitates, expected) in [
            (ImitatedGrid::OpenSim, ""),
            (ImitatedGrid::SecondLife, "video/mp4"),
        ] {
            let mut parcel = parcel_with_media();
            let udp = ParcelUpdate {
                local_id: parcel.local_id,
                media_data: None,
                ..parcel.to_update()
            };
            apply_update(&mut parcel, &udp, imitates.parcel_policy());
            assert_eq!(
                parcel.media_data.map(|media| media.media_type),
                Some(expected.to_owned()),
                "{imitates:?}"
            );
        }
    }

    /// A region-wide parcel owned by [`land_owner`] holding one box of the land
    /// owner's and one of a resident's, as a [`ReturnRequest`] sees them.
    fn returnable_scene() -> (SceneFixtures, sl_proto::Object, sl_proto::Object) {
        let mut world = SceneFixtures::new();
        world.parcels.push(region_wide_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(land_owner()),
            "Land",
        ));
        let at = |x: f32| sl_proto::Vector {
            x,
            y: 128.0,
            z: 25.0,
        };
        let unit = sl_proto::Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        let owners = box_prim(
            sl_proto::RegionLocalObjectId(10),
            ObjectKey::from(uuid::Uuid::from_u128(10)),
            land_owner(),
            at(100.0),
            unit.clone(),
        );
        let residents = box_prim(
            sl_proto::RegionLocalObjectId(11),
            ObjectKey::from(uuid::Uuid::from_u128(11)),
            resident(),
            at(140.0),
            unit,
        );
        world.objects.push(owners.clone());
        world.objects.push(residents.clone());
        (world, owners, residents)
    }

    /// The owner of the region-wide parcel in [`returnable_scene`].
    fn land_owner() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(1))
    }

    /// The resident whose box stands on [`land_owner`]'s parcel.
    fn resident() -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(2))
    }

    /// A return on a parcel takes the classes its type names, measured against
    /// the parcel's owner — on both flavours.
    #[test]
    fn a_parcel_return_takes_the_classes_its_type_names() {
        let (world, owners, residents) = returnable_scene();
        for imitates in [ImitatedGrid::OpenSim, ImitatedGrid::SecondLife] {
            let policy = imitates.parcel_policy();
            let takes = |return_type, owner_ids: &[OwnerKey], object: &sl_proto::Object| {
                let request = ReturnRequest {
                    local_id: RegionLocalParcelId(1),
                    return_type,
                    task_ids: &[],
                    owner_ids,
                };
                returned(&world, object, &request, policy)
            };
            assert!(takes(ParcelReturnType::OWNER, &[], &owners), "{imitates:?}");
            assert!(
                !takes(ParcelReturnType::OWNER, &[], &residents),
                "{imitates:?}"
            );
            assert!(
                takes(ParcelReturnType::OTHER, &[], &residents),
                "{imitates:?}"
            );
            assert!(
                !takes(ParcelReturnType::OTHER, &[], &owners),
                "{imitates:?}"
            );
            let named_owner = [OwnerKey::Agent(resident())];
            assert!(
                takes(ParcelReturnType::LIST, &named_owner, &residents),
                "{imitates:?}"
            );
            assert!(
                !takes(ParcelReturnType::LIST, &named_owner, &owners),
                "{imitates:?}"
            );
        }
    }

    /// Naming one object in a return addressed to a parcel takes it on the
    /// Second Life flavour and nothing on OpenSim's, which never reads the task
    /// list there; addressed to the whole region, both take it.
    #[test]
    fn only_a_whole_region_return_reads_the_task_list_on_opensim() {
        let (world, _, residents) = returnable_scene();
        let named = [residents.full_id];
        for (imitates, on_a_parcel) in [
            (ImitatedGrid::OpenSim, false),
            (ImitatedGrid::SecondLife, true),
        ] {
            let policy = imitates.parcel_policy();
            for (local_id, expected) in
                [(RegionLocalParcelId(1), on_a_parcel), (WHOLE_REGION, true)]
            {
                let request = ReturnRequest {
                    local_id,
                    return_type: ParcelReturnType::NONE,
                    task_ids: &named,
                    owner_ids: &[],
                };
                assert_eq!(
                    returned(&world, &residents, &request, policy),
                    expected,
                    "{imitates:?} on {local_id:?}"
                );
            }
        }
    }

    /// The capability form carries the media block, and both flavours keep it.
    #[test]
    fn a_capability_edit_keeps_the_media_type_on_both_flavours() {
        for imitates in [ImitatedGrid::OpenSim, ImitatedGrid::SecondLife] {
            let mut parcel = parcel_with_media();
            let edit = parcel.to_update();
            apply_update(&mut parcel, &edit, imitates.parcel_policy());
            assert_eq!(
                parcel.media_data.map(|media| media.media_type),
                Some("video/mp4".to_owned()),
                "{imitates:?}"
            );
        }
    }
}
