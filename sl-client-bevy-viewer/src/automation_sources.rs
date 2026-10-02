//! The viewer's **probe sources**: the readers
//! [`sl_viewer_automation::ProbeSources`] names for the models that live in
//! the viewer's heavy crates, registered by the assembly so a state probe can
//! read every model the whole viewer keeps.
//!
//! Registering them costs nothing: a reader runs only when a probe asks.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sl_viewer_automation::{FileDialogAnswer, ProbeSources, SceneEnvironment, SceneWater};
use sl_viewer_platform::file_dialog::{
    FileDialogBackend, FileDialogSelection, answer_pending_dialog,
};
use sl_viewer_world_scene::environment::EnvironmentState;
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
        file_dialog: Some(answer_file_dialog),
        environment_scene: Some(environment_scene),
        published_bakes: Some(published_bakes),
    }
}

/// Forward each UI sound the viewer raises to the automation event log's sound
/// stream, by the sound's name.
pub(crate) fn log_ui_sounds(
    mut raised: MessageReader<sl_viewer_ui_sounds::ui_sounds::PlayUiSound>,
    mut logged: MessageWriter<sl_viewer_automation::SoundRaised>,
) {
    for sound in raised.read() {
        logged.write(sl_viewer_automation::SoundRaised(sound.0.key()));
    }
}

/// The scene's environment beyond the sky the RLV slot publishes: the water
/// drawn, a cross-fade under way, the windows previewing.
fn environment_scene(world: &mut World) -> SceneEnvironment {
    let Some(state) = world.get_resource::<EnvironmentState>() else {
        return SceneEnvironment::default();
    };
    SceneEnvironment {
        transition: state.transition_fraction(),
        water: state
            .water_at(sl_viewer_world_scene::sky::day_position(state))
            .map(|water| SceneWater {
                name: water.name,
                fog_density: water.water_fog_density,
            }),
        previewing: state
            .previewers()
            .map(|previewer| previewer.0.to_owned())
            .collect(),
    }
}

/// Answer the file dialog the viewer waits on with `picked`, or Cancel — when
/// the viewer waits on an answer at all rather than showing the desktop's
/// chooser.
fn answer_file_dialog(world: &mut World, picked: Option<PathBuf>) -> FileDialogAnswer {
    if world.get_resource::<FileDialogBackend>() != Some(&FileDialogBackend::Answered) {
        return FileDialogAnswer::ShownOnDesktop;
    }
    answer_pending_dialog(world, picked).map_or(FileDialogAnswer::NothingWaiting, |answered| {
        FileDialogAnswer::Answered {
            purpose: answered.purpose.into(),
            title: answered.title,
            folder: answered.selection == FileDialogSelection::Folder,
        }
    })
}

/// The baked textures the own avatar's client-side bake published.
fn published_bakes(world: &mut World) -> Vec<sl_client_bevy::Uuid> {
    world
        .get_resource::<sl_viewer_world_avatar::bake_publish::OwnBakePublish>()
        .map(sl_viewer_world_avatar::bake_publish::OwnBakePublish::published)
        .unwrap_or_default()
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
