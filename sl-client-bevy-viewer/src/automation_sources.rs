//! The viewer's **probe sources**: the readers
//! [`sl_viewer_automation::ProbeSources`] names for the models that live in
//! the viewer's heavy crates, registered by the assembly so a state probe can
//! read every model the whole viewer keeps.
//!
//! Registering them costs nothing: a reader runs only when a probe asks.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sl_viewer_automation::ProbeSources;
use sl_viewer_world_view::quiescence::SceneQuiescence;

use crate::status_bar::AgentBalance;

/// Every probe source the whole viewer has.
pub(crate) fn probe_sources() -> ProbeSources {
    ProbeSources {
        conversations: Some(sl_viewer_people::conversations::conversation_readouts),
        live_notifications: Some(sl_viewer_notices::notification_host::live_notifications),
        teleport: Some(sl_viewer_places::teleport_progress::teleport_readout),
        balance: Some(balance),
        scene_work: Some(scene_work),
    }
}

/// The own L$ balance as the status bar shows it.
fn balance(world: &mut World) -> Option<i64> {
    world
        .get_resource::<AgentBalance>()
        .and_then(AgentBalance::linden_dollars)
}

/// The scene's outstanding work by bucket, across every asset store and
/// queue — what the screenshot mode and the full-stack harness wait on.
fn scene_work(world: &mut World) -> BTreeMap<String, u64> {
    let mut state = SystemState::<SceneQuiescence<'_, '_>>::new(world);
    state.get(world).map_or_else(
        |_error| BTreeMap::new(),
        |scene| {
            scene
                .breakdown()
                .into_iter()
                .map(|(bucket, count)| (bucket, u64::try_from(count).unwrap_or(u64::MAX)))
                .collect()
        },
    )
}
