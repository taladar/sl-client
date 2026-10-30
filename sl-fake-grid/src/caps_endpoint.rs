//! The CAPS side of the HTTP service: routing one collected request into
//! [`SimCaps`] / the asset caps, including the `EventQueueGet` long-poll
//! hold the sans-I/O core deliberately leaves to its runtime.

use std::sync::Arc;

use sl_proto::{
    CapsDispatch, CapsRequest, CapsResponse, ModifyRegionOutcome, SimCaps, UntrustedMessage,
    UntrustedMessageOutcome,
};

use crate::driver::{SharedSim, SimState, sleep_until_opt};
use crate::runtime::GridCore;

/// The request headers a capability reads: the asset-delivery ones, and the
/// `Destination` of an AIS3 `COPY`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CapsHeaders<'a> {
    /// The `Range` header.
    pub(crate) range: Option<&'a str>,
    /// The `If-Modified-Since` header.
    pub(crate) if_modified_since: Option<&'a str>,
    /// The `Destination` header.
    pub(crate) destination: Option<&'a str>,
}

/// Dispatches one CAPS request against a session, holding an empty
/// `EventQueueGet` poll for up to `eq_hold` before answering the 502 the
/// client reads as "nothing yet, re-poll". The hold also ends the moment the
/// grid shuts down, so teardown is never held hostage by a poll that would
/// otherwise sit for the whole `eq_hold`.
///
/// Asset-delivery caps (`GetTexture`, `GetMesh`, `ViewerAsset`, …) route to
/// the session-free asset surface against the scenario's asset store;
/// everything else goes through [`sl_proto::SimCaps::dispatch`] followed by
/// the driver's flush rule (a caps POST can queue transmits and events).
pub(crate) async fn dispatch_caps(
    core: &GridCore,
    shared: &SharedSim,
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: CapsHeaders<'_>,
    body: &[u8],
) -> CapsResponse {
    // The hold deadline covers the whole poll, not each re-dispatch.
    let deadline = shared.now().checked_add(core.eq_hold);
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        let request = CapsRequest {
            method,
            path,
            query,
            range: headers.range,
            if_modified_since: headers.if_modified_since,
            destination: headers.destination,
            body,
        };
        let dispatch = {
            let mut guard = shared.state.lock().await;
            if guard.caps.assets().handles_path(path) {
                // Session-free binary asset serving out of the **grid-wide**
                // store: no sim state touched, and no region's content is
                // unreachable from another region's capability. The read guard
                // never outlives this block, so no await ever happens under it.
                let store = guard.assets.read();
                return guard.caps.assets().dispatch(&*store, &request);
            }
            let SimState { sim, caps, .. } = &mut *guard;
            let dispatch = caps.dispatch(sim, &request);
            let outcome = shared.flush_locked(&mut guard);
            drop(guard);
            shared.finish_flush(outcome).await;
            dispatch
        };
        match dispatch {
            CapsDispatch::Response(response) => return response,
            // A prim's contents are the region's: the listing is read out of
            // the world the session shares with its neighbours.
            CapsDispatch::TaskInventory(query) => {
                let world = Arc::clone(&shared.state.lock().await.world);
                let world = world.lock();
                let contents = world.local_id_of(query.task).map(|local_id| {
                    world
                        .task_inventories
                        .get(&local_id)
                        .map_or((0, &[][..]), |held| (held.serial, held.items.as_slice()))
                });
                let response = SimCaps::task_inventory_response(query, contents);
                drop(world);
                return response;
            }
            // The compiler is the grid's, and what a compile does to the
            // script is the region's.
            CapsDispatch::ScriptUpload(upload) => {
                let world = Arc::clone(&shared.state.lock().await.world);
                let answer = crate::script_compile::answer_upload(&mut world.lock(), &upload);
                return SimCaps::script_upload_response(&answer);
            }
            CapsDispatch::ScriptMetadata { task, item } => {
                let world = Arc::clone(&shared.state.lock().await.world);
                let script = world.lock().script_experience(task, item);
                return SimCaps::script_metadata_response(script);
            }
            // A notecard and what it embeds are the session's inventory and
            // the grid's store; what is filed, and the answer, the grid's.
            CapsDispatch::CopyFromNotecard {
                notecard_id,
                object_id,
                item_id,
                folder_id,
            } => {
                let mut guard = shared.state.lock().await;
                let now = shared.now();
                let answer = {
                    let state = &mut *guard;
                    let minter = state.minter.clone();
                    crate::notecards::copy_from_notecard(
                        &mut state.sim,
                        &state.assets,
                        crate::notecards::CopyRequest {
                            notecard: notecard_id,
                            object: object_id,
                            item: item_id,
                            folder: folder_id,
                        },
                        &move || minter.uuid(),
                        now,
                    )
                };
                let outcome = shared.flush_locked(&mut guard);
                drop(guard);
                shared.finish_flush(outcome).await;
                return SimCaps::copy_from_notecard_response(answer);
            }
            // The groups are the grid's.
            CapsDispatch::GroupMembers { group_id } => {
                let members = crate::groups::roster_for_capability(core, group_id).await;
                return SimCaps::group_member_data_response(group_id, &members);
            }
            CapsDispatch::GroupBans {
                group_id,
                change: None,
            } => {
                return SimCaps::group_bans_response(
                    group_id,
                    &crate::groups::bans_of(core, group_id),
                );
            }
            CapsDispatch::GroupBans {
                group_id,
                change: Some((action, agents)),
            } => {
                crate::groups::change_bans(core, group_id, action, &agents);
                return SimCaps::group_bans_changed_response(action);
            }
            // What the viewers report of each other is the region's.
            CapsDispatch::AvatarRenderInfoRead { reader } => {
                let world = Arc::clone(&shared.state.lock().await.world);
                let statement = world.lock().render_info.statement(reader, shared.now());
                return SimCaps::avatar_render_info_response(&statement);
            }
            CapsDispatch::AvatarRenderInfoReport {
                reporter: _,
                reports,
            } => {
                let world = Arc::clone(&shared.state.lock().await.world);
                world.lock().render_info.file(&reports, shared.now());
                return SimCaps::avatar_render_report_response();
            }
            // The estate is the region's, and who may read it the session's:
            // a manager reads OpenSim's arrays on either flavour (Second
            // Life's manager answer is unmeasured), a resident Second Life's
            // four `undef` lists or OpenSim's `401`.
            CapsDispatch::EstateAccess => {
                let guard = shared.state.lock().await;
                let lists = guard.world.lock().estate.access_lists();
                let style = if guard.policy.estate_manager {
                    Some(sl_proto::EstateAccessStyle::Arrays)
                } else if guard.estate_policy.access_capability_refuses_residents {
                    None
                } else {
                    Some(sl_proto::EstateAccessStyle::Undefined)
                };
                drop(guard);
                return SimCaps::estate_access_response(style.map(|style| (&lists, style)));
            }
            // The region's settings are the region's, and who may write them
            // the session's: a manager's write is applied as `setregioninfo`
            // would be, and the region's other sessions are told.
            CapsDispatch::RegionInfoWrite(update) => {
                let mut guard = shared.state.lock().await;
                if !guard.policy.estate_manager {
                    drop(guard);
                    return SimCaps::region_info_write_response(false);
                }
                let now = shared.now();
                {
                    let state = &mut *guard;
                    let world = Arc::clone(&state.world);
                    let mut world = world.lock();
                    let changes = crate::estate::apply_region_info(
                        &mut world,
                        &state.identity,
                        &update,
                        &mut state.sim,
                        now,
                    );
                    drop(world);
                    for change in changes {
                        drop(state.changes.send(crate::world::RegionUpdate {
                            source: state.seq,
                            change,
                        }));
                    }
                }
                let outcome = shared.flush_locked(&mut guard);
                drop(guard);
                shared.finish_flush(outcome).await;
                return SimCaps::region_info_write_response(true);
            }
            // The restart schedule is the region's; a manager may write it.
            CapsDispatch::RegionSchedule { change } => {
                let guard = shared.state.lock().await;
                let manager = guard.policy.estate_manager;
                let world = Arc::clone(&guard.world);
                drop(guard);
                if change.is_some() && !manager {
                    return SimCaps::region_schedule_response(None);
                }
                let mut world = world.lock();
                if let Some(schedule) = change {
                    world.restart_schedule = schedule;
                }
                return SimCaps::region_schedule_response(Some(world.restart_schedule.as_ref()));
            }
            // The terrain overrides are the region's; a manager may write
            // them.
            CapsDispatch::ModifyRegion { change } => {
                let guard = shared.state.lock().await;
                let manager = guard.policy.estate_manager;
                let region = guard.sim.region_id();
                let world = Arc::clone(&guard.world);
                drop(guard);
                if change.is_some() && !manager {
                    return SimCaps::modify_region_response(ModifyRegionOutcome::Refused(region));
                }
                let mut world = world.lock();
                return match change {
                    Some(overrides) => {
                        world.terrain_material_overrides = overrides;
                        SimCaps::modify_region_response(ModifyRegionOutcome::Written)
                    }
                    None => SimCaps::modify_region_response(ModifyRegionOutcome::Read(
                        &world.terrain_material_overrides,
                    )),
                };
            }
            // A template message posted as LLSD: a god kick is checked as the
            // grid checks it and answered in its words; what the kick then does
            // is the roadmap's `server-fake-grid-god-tools`.
            CapsDispatch::UntrustedMessage(message) => {
                let guard = shared.state.lock().await;
                let style = guard.untrusted_style;
                let outcome = match message {
                    UntrustedMessage::GodKickUser(kick) => {
                        let presence = guard.sim.agent_id() == Some(kick.god)
                            && guard.sim.session_id() == Some(kick.god_session);
                        if presence || style == sl_proto::UntrustedMessageStyle::Llsd {
                            UntrustedMessageOutcome::Taken
                        } else {
                            UntrustedMessageOutcome::WrongSession
                        }
                    }
                    UntrustedMessage::Other(_) => UntrustedMessageOutcome::Unknown,
                };
                drop(guard);
                return SimCaps::untrusted_message_response(style, outcome);
            }
            // "Set Home to Here" over the capability: the grid's set-home rule,
            // what it says beside the answer, and the answer in its words.
            CapsDispatch::HomeLocation(asked) => {
                use crate::agent_requests::{HomeStore, SetHome, set_home, tell};
                use sl_proto::HomeLocationOutcome;
                let mut guard = shared.state.lock().await;
                let now = shared.now();
                let state = &mut *guard;
                let policy = state.home_policy;
                let manager = state.policy.estate_manager;
                let Some(agent) = state.sim.agent_id() else {
                    drop(guard);
                    return SimCaps::home_location_response(
                        policy.capability,
                        HomeLocationOutcome::NoLocation,
                    );
                };
                let (outcome, answer) = match &asked {
                    None => (SetHome::NoLocation, HomeLocationOutcome::NoLocation),
                    Some(asked) => {
                        let outcome = set_home(
                            manager,
                            HomeStore {
                                homes: &state.homes,
                                region_id: state.identity.region_id,
                                policy: &policy,
                            },
                            &state.world.lock(),
                            agent,
                            asked.position,
                            &asked.look_at,
                        );
                        let answer = if outcome == SetHome::Stored {
                            HomeLocationOutcome::Stored(asked.position)
                        } else {
                            HomeLocationOutcome::Refused(asked.position)
                        };
                        (outcome, answer)
                    }
                };
                let quiet = policy.repeat_quiet;
                tell(
                    &mut state.sim,
                    agent,
                    outcome.notice(&policy.capability_notices),
                    (quiet, &mut state.home_told),
                    now,
                );
                let flushed = shared.flush_locked(&mut guard);
                drop(guard);
                shared.finish_flush(flushed).await;
                return SimCaps::home_location_response(policy.capability, answer);
            }
            CapsDispatch::EventQueueWouldBlock => {
                if *shutdown_rx.borrow_and_update() {
                    let guard = shared.state.lock().await;
                    return guard.caps.event_queue_timeout();
                }
                // A hold with no deadline waits for the wakeup (or the
                // shutdown) alone.
                let expired = tokio::select! {
                    () = shared.eq_notify.notified() => false,
                    () = sleep_until_opt(deadline) => true,
                    changed = shutdown_rx.changed() => changed.is_err() || *shutdown_rx.borrow(),
                };
                if expired {
                    let guard = shared.state.lock().await;
                    return guard.caps.event_queue_timeout();
                }
                // Woken: loop back and re-dispatch against the fresh queue.
            }
        }
    }
}
