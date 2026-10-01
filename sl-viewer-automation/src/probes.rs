//! **State probes**: what a test asserts on that is not one widget, read from
//! the models the viewer already keeps — never scraped from the widgets that
//! draw them, and never computed per frame.
//!
//! Each reader takes the world when asked. The models of this crate's own
//! dependencies are read directly; the ones that live in the viewer's heavy
//! crates come through [`ProbeSources`], which the viewer's assembly fills.
//! The streams — the [`EventLog`](crate::EventLog) and the diagnostics
//! ([`read_diagnostics`](crate::read_diagnostics)) — are read by cursor.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sl_automation_proto::{
    AgentReadout, CameraView, ClockTime, ConversationReadout, EnvironmentReadout, InventoryEntry,
    InventoryFolderReadout, InventoryRoot, NotificationReadout, OfferedButton, QuiescenceReadout,
    RegionReadout, SelectedObject, SkyReadout, StatusReadout,
};
use sl_client_bevy::{
    FolderState, InventoryFolderKey, SlAgentParcel, SlCurrentRegion, SlIdentity, SlRegion,
    SlRegionIdentity,
};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_kit::slt;
use sl_viewer_notifications::{NotificationManager, NotificationRecord, ToastButton, template};
use sl_viewer_ui_core::i18n::Translator;
use sl_viewer_world_api::rlv::RlvEnvironmentSlot;
use sl_viewer_world_api::{AgentRegionPosition, AvatarControls, CameraMode, SelectionSet};

use crate::probe_sources::ProbeSources;
use crate::render_settle::PipelineStatus;

/// Why a probe could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// The app keeps no such model: a partial app, or a viewer whose assembly
    /// registered no source for it.
    #[error("this viewer has no {probe} to read")]
    Unavailable {
        /// What was asked for.
        probe: &'static str,
    },
    /// An inventory path names a folder the viewer does not know.
    #[error("no inventory folder {segment:?} at {path:?} (segment {index})")]
    NoSuchFolder {
        /// The whole path asked for.
        path: Vec<String>,
        /// The index of the first segment that named nothing.
        index: usize,
        /// That segment.
        segment: String,
    },
}

/// Every open conversation and its transcript, Nearby first.
///
/// # Errors
///
/// [`ProbeError::Unavailable`] when the app keeps no conversation model.
pub fn read_conversations(world: &mut World) -> Result<Vec<ConversationReadout>, ProbeError> {
    let reader = ProbeSources::of(world)
        .conversations
        .ok_or(ProbeError::Unavailable {
            probe: "conversation model",
        })?;
    Ok(reader(world))
}

/// Every notification still in the viewer's history, oldest first: its text,
/// the buttons it offers, whether it is on screen now and how it was answered.
///
/// A live one reports the buttons its card shows — a script dialog's own, a
/// permission request's — and one no longer shown the buttons its catalogue
/// template names (labelled in the current locale), where it has one. The
/// history is bounded
/// ([`HISTORY_CAP`](sl_viewer_notifications::HISTORY_CAP)); an app without
/// the notification host keeps none and reads empty.
#[must_use]
pub fn read_notifications(world: &mut World) -> Vec<NotificationReadout> {
    let live = ProbeSources::of(world)
        .live_notifications
        .map_or_else(Vec::new, |reader| reader(world));
    let records: Vec<NotificationRecord> = world
        .get_resource::<NotificationManager>()
        .map_or_else(Vec::new, |manager| manager.history().cloned().collect());
    let mut translator = SystemState::<Translator<'_>>::new(world);
    let translator = translator.get(world).ok();
    let offered = |button: &ToastButton| OfferedButton {
        name: button.name.clone(),
        label: button.label.clone(),
        default: button.is_default,
    };
    records
        .into_iter()
        .map(|record| {
            let shown = live
                .iter()
                .find(|(id, _buttons)| *id == record.id)
                .map(|(_id, buttons)| buttons.iter().map(offered).collect::<Vec<_>>());
            let buttons = shown.clone().unwrap_or_else(|| {
                template(record.template).map_or_else(Vec::new, |entry| {
                    entry
                        .form
                        .iter()
                        .map(|button| OfferedButton {
                            name: button.name.to_owned(),
                            label: translator.as_ref().map_or_else(
                                || button.label_key.to_owned(),
                                |translator| translator.get(button.label_key),
                            ),
                            default: button.is_default,
                        })
                        .collect()
                })
            });
            NotificationReadout {
                id: record.id.get(),
                template: record.template.to_owned(),
                text: record.body,
                buttons,
                live: shown.is_some(),
                response: record.response.map(ToOwned::to_owned),
            }
        })
        .collect()
}

/// The current region's entity parts: its handle and, once the handshake
/// arrived, its identity.
fn current_region(world: &mut World) -> Option<RegionReadout> {
    let mut regions =
        world.query_filtered::<(&SlRegion, Option<&SlRegionIdentity>), With<SlCurrentRegion>>();
    let (region, identity) = regions.iter(world).next()?;
    Some(RegionReadout {
        name: identity
            .and_then(|identity| identity.0.sim_name.as_ref())
            .map(ToString::to_string),
        handle: region.handle.get(),
        id: identity.map(|identity| identity.0.region_id),
    })
}

/// What the status bar shows: the region, the parcel, the balance and the
/// time — read from the models it is drawn from.
#[must_use]
pub fn read_status(world: &mut World) -> StatusReadout {
    let now = slt::current_slt(slt::now_unix());
    StatusReadout {
        region: current_region(world).and_then(|region| region.name),
        parcel: world
            .get_resource::<SlAgentParcel>()
            .and_then(|parcel| parcel.current.as_ref())
            .map(|parcel| parcel.name.clone())
            .filter(|name| !name.is_empty()),
        balance: ProbeSources::of(world)
            .balance
            .and_then(|reader| reader(world)),
        time: ClockTime {
            hour: now.hour,
            minute: now.minute,
        },
    }
}

/// The own agent: who, where, on what, how far through a teleport, how the
/// camera is driven and which way the avatar faces.
#[must_use]
pub fn read_agent(world: &mut World) -> AgentReadout {
    let teleport = ProbeSources::of(world)
        .teleport
        .and_then(|reader| reader(world));
    AgentReadout {
        agent_id: world
            .get_resource::<SlIdentity>()
            .and_then(|identity| identity.agent_id)
            .map(|agent| agent.uuid()),
        region: current_region(world),
        position: world
            .get_resource::<AgentRegionPosition>()
            .and_then(|position| position.position.as_ref())
            .map(|at| [at.x, at.y, at.z]),
        seated_on: world
            .get_resource::<SlAgentParcel>()
            .and_then(|parcel| parcel.seated_on)
            .map(|seat| seat.uuid()),
        teleport,
        camera: world.get_resource::<CameraMode>().map(|mode| match mode {
            CameraMode::ThirdPerson => CameraView::ThirdPerson,
            CameraMode::Mouselook => CameraView::Mouselook,
            CameraMode::Flycam => CameraView::Flycam,
        }),
        heading: world
            .get_resource::<AvatarControls>()
            .and_then(AvatarControls::held_heading),
    }
}

/// The environment being drawn: the sky the scene publishes for RLV's
/// `@getenv_*` — which is the one it renders — and whether the viewer's own
/// local sky stands in for the shared one. An app without the environment
/// scene has drawn no sky and has no local one.
#[must_use]
pub fn read_environment(world: &World) -> EnvironmentReadout {
    let slot = world.get_resource::<RlvEnvironmentSlot>();
    EnvironmentReadout {
        sky: slot
            .and_then(|slot| slot.rendered.as_ref())
            .map(|sky| SkyReadout {
                name: sky.name.clone(),
                ambient: [sky.ambient.red(), sky.ambient.green(), sky.ambient.blue()],
            }),
        local_sky: slot.is_some_and(|slot| slot.fixed_sky),
    }
}

/// The edit selection in selection order, the primary last.
#[must_use]
pub fn read_selection(world: &World) -> Vec<SelectedObject> {
    let Some(selection) = world.get_resource::<SelectionSet>() else {
        return Vec::new();
    };
    let count = selection.len();
    selection
        .iter()
        .enumerate()
        .map(|(index, node)| SelectedObject {
            full_id: node.full.uuid(),
            local_id: node.scoped().id.0,
            primary: index.saturating_add(1) == count,
        })
        .collect()
}

/// The inventory folder at `path` under `root` — each segment a sub-folder's
/// exact name, the first match where two share one — and what the viewer
/// knows of its contents. The empty path is the root itself.
///
/// # Errors
///
/// [`ProbeError::Unavailable`] when the app has no inventory model, and
/// [`ProbeError::NoSuchFolder`] naming the first segment the viewer knows no
/// folder for (the root itself, before the skeleton arrived).
pub fn read_inventory(
    world: &World,
    root: InventoryRoot,
    path: &[String],
) -> Result<InventoryFolderReadout, ProbeError> {
    let model = world
        .get_resource::<InventoryModel>()
        .ok_or(ProbeError::Unavailable {
            probe: "inventory model",
        })?;
    let missing = |index: usize, segment: &str| ProbeError::NoSuchFolder {
        path: path.to_vec(),
        index,
        segment: segment.to_owned(),
    };
    let root_folder = match root {
        InventoryRoot::Agent => model.agent_root(),
        InventoryRoot::Library => model.library_root(),
    }
    .ok_or_else(|| missing(0, ""))?;
    let mut folder = root_folder;
    for (index, segment) in path.iter().enumerate() {
        folder = model
            .child_folders_of(folder)
            .iter()
            .copied()
            .find(|child| {
                model
                    .folder_info(*child)
                    .is_some_and(|info| info.name == *segment)
            })
            .ok_or_else(|| missing(index, segment))?;
    }
    Ok(folder_readout(model, folder))
}

/// One folder of `model` and its known contents.
fn folder_readout(model: &InventoryModel, folder: InventoryFolderKey) -> InventoryFolderReadout {
    let info = model.folder_info(folder);
    InventoryFolderReadout {
        id: folder.uuid(),
        name: info.map(|info| info.name.clone()).unwrap_or_default(),
        loaded: info.is_some_and(|info| matches!(info.state, FolderState::Loaded { .. })),
        folders: model
            .child_folders_of(folder)
            .iter()
            .filter_map(|child| model.folder_info(*child))
            .map(|child| InventoryEntry {
                id: child.folder_id.uuid(),
                name: child.name.clone(),
                kind: snake_case(&format!("{:?}", child.folder_type)),
            })
            .collect(),
        items: model
            .loaded_items_of(folder)
            .iter()
            .map(|item| InventoryEntry {
                id: item.item_id.uuid(),
                name: item.name.clone(),
                kind: snake_case(&format!("{:?}", item.asset_type)),
            })
            .collect(),
    }
}

/// A `Debug` spelling of a unit variant in snake case: `CurrentOutfit` is
/// `current_outfit`, `Unknown(7)` is `unknown(7)`.
fn snake_case(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len().saturating_add(4));
    let mut previous_lower = false;
    for character in debug.chars() {
        if character.is_uppercase() {
            if previous_lower {
                out.push('_');
            }
            out.extend(character.to_lowercase());
            previous_lower = false;
        } else {
            out.push(character);
            previous_lower = character.is_lowercase() || character.is_ascii_digit();
        }
    }
    out
}

/// Whether the viewer has settled: the region is up, nothing the scene asked
/// for is outstanding, and no render pipeline is compiling.
#[must_use]
pub fn read_quiescence(world: &mut World) -> QuiescenceReadout {
    let region_up = world
        .query_filtered::<(), With<SlCurrentRegion>>()
        .iter(world)
        .next()
        .is_some();
    let outstanding_by = ProbeSources::of(world)
        .scene_work
        .map(|reader| reader(world));
    QuiescenceReadout {
        region_up,
        outstanding: outstanding_by
            .as_ref()
            .map(|buckets| buckets.values().copied().fold(0_u64, u64::saturating_add)),
        outstanding_by: outstanding_by.unwrap_or_default(),
        waiting_pipelines: world
            .get_resource::<PipelineStatus>()
            .map(PipelineStatus::waiting),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::snake_case;

    #[test]
    fn debug_spellings_become_snake_case() {
        assert_eq!(snake_case("CurrentOutfit"), "current_outfit");
        assert_eq!(snake_case("Notecard"), "notecard");
        assert_eq!(snake_case("LSLText"), "lsltext");
        assert_eq!(snake_case("Unknown(7)"), "unknown(7)");
    }
}
