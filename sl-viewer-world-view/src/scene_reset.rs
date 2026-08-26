//! Purge the viewer's scene mirror on a **distant** teleport.
//!
//! A region *crossing* and a teleport to an *already-connected* (neighbour)
//! region keep the whole world and merely re-base it onto the new origin (see
//! [`sl_viewer_world_objects::objects::recenter_objects`] / [`sl_viewer_world_objects::avatars::recenter_avatars`] /
//! [`sl_viewer_world_scene::terrain::recenter_terrain`]). A **distant** teleport instead mints a
//! fresh circuit to an unconnected region: the session clears its object / terrain
//! / region caches with *no* per-object `KillObject`, so nothing drives the
//! incremental despawn path and the old region's entities would linger forever,
//! piled at their stale offsets.
//!
//! [`reset_scene_on_world_reset`] reacts to
//! [`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset` flag
//! (set by the session only on that fresh-circuit branch) and purges the object,
//! avatar and terrain mirrors — the destination then streams everything fresh,
//! exactly as a login does. The agent's **own** avatar and its worn attachments
//! are kept (they cross with the agent), so the self view does not flash.

use bevy::prelude::*;
use sl_client_bevy::{SlEvent, SlIdentity, SlSessionEvent};

use crate::objects::PendingObjectEvents;
use crate::terrain::TerrainTextures;
use crate::world_api::AvatarState;
use crate::world_api::ObjectState;
use crate::world_api::TerrainState;

/// On a distant-teleport `world_reset`, despawn the world-object, avatar and
/// terrain mirrors (keeping the own avatar + attachments) so the stale
/// old-region scene does not linger. A no-op for a crossing / neighbour teleport
/// (their `RegionChanged` carries `world_reset == false`, and the recenter
/// systems re-base the kept world instead).
///
/// Runs before the recenter systems so each subsystem's origin, dropped to `None`
/// by its purge, is re-anchored on the destination without a spurious re-base
/// shift.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and event \
              readers; one purge per store the distant teleport clears"
)]
pub fn reset_scene_on_world_reset(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut objects: ResMut<ObjectState>,
    mut pending: ResMut<PendingObjectEvents>,
    mut avatars: ResMut<AvatarState>,
    mut terrain: ResMut<TerrainState>,
    mut terrain_textures: ResMut<TerrainTextures>,
    mut commands: Commands,
) {
    // Only the last reset in a frame matters (they all purge the same scene);
    // fold the stream to a single flag so a burst never purges twice.
    let mut reset = false;
    for event in events.read() {
        if let SlSessionEvent::RegionChanged {
            world_reset: true, ..
        } = &event.0
        {
            reset = true;
        }
    }
    if !reset {
        return;
    }
    // The object mirror is purged wholesale (the own avatar's *object* entity
    // included — it is only a position mirror); the agent's visible body is kept
    // by `AvatarState::purge` (agent-keyed, so it does not flash) and the
    // destination re-streams everything.
    objects.purge(&mut commands);
    // The purged objects' deferred geometry builds are components on the entities
    // that purge just despawned, so they go with them.
    pending.clear();
    avatars.purge(identity.agent_id, &mut commands);
    terrain.purge(&mut commands);
    // The region materials belonged to the regions that purge just dropped; the
    // shared placeholder and the decoded detail textures are kept.
    terrain_textures.purge_materials();
}
