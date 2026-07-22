//! The **land / terrain** context / pie menu (`viewer-land-context-menu`): the
//! entry set offered when bare terrain is the pick target, and the dispatch of
//! each entry.
//!
//! This is the *entries*, not the widget — the radial widget is
//! [`crate::pie_menu`], and this module declares the [`PieMenuDef`] plus the
//! systems that open and act on it, exactly as [`crate::avatar_menu`] and
//! [`crate::object_menu`] do for their targets. The tree reproduces the
//! reference viewer's `menu_pie_land.xml` (shared by every skin — Vintage
//! overrides none) at the reference compass positions: reference slice order
//! maps East → NorthEast → … → SouthEast, the same slot convention every other
//! pie here follows.
//!
//! # What is wired, and what is a disabled placeholder
//!
//! Most of the reference's land actions belong to features this viewer does not
//! have yet. Those sit **in their reference compass positions but disabled**,
//! gated on the never-supplied [`UNIMPLEMENTED`] sentinel, so the menu's shape
//! (the muscle memory) is laid down now and each slice lights up when its
//! feature lands — one `when` edit, address unchanged:
//!
//! - **About Land…** waits for a parcel-information surface
//!   (`viewer-parcel-options-general`); the parcel *data* is already modelled.
//! - **Create** and **Edit Terrain** wait for the build / terraform tools.
//! - **Go Here** waits for autopilot.
//! - **Mute Part. Own.** waits for particle picking
//!   (`viewer-particle-pick-mute`), like the object pie's twin slice.
//! - **Buy Pass** and **Buy This Land** wait for the land-buy flows.
//!
//! Wired for real: **Sit Here** → ground sit ([`Command::SitOnGround`]),
//! standing the avatar up first when it is object-seated, as the reference's
//! `Land.Sit` does. The reference then *autopilots* to the clicked point and
//! sits there; without autopilot (`Go Here` above) ours sits in place — the
//! deliberate simplification, upgraded when autopilot lands.
//!
//! # How a pick reaches here
//!
//! [`crate::avatar_menu`]'s right-click resolver owns the gesture and the
//! occlusion order (UI, then HUD, then world). In the world it resolves the ray
//! against avatars, objects, **and** the terrain ([`pick_land`]): terrain is
//! deliberately last in priority at equal distance, but a hill *in front of*
//! an avatar or object wins by distance, exactly like the reference's unified
//! first-hit pick. The terrain patches are the [`TerrainSurface`] meshes the
//! avatar ground probe ([`crate::ground`]) already raycasts.
//!
//! Reference (Firestorm, read-only): `menu_pie_land.xml` (the compass
//! positions), `lltoolpie.cpp` (the land pick), `llviewermenu.cpp`
//! (`LLLandSit` and friends).

use std::collections::HashSet;

use bevy::prelude::*;
use sl_client_bevy::{Command, SlAgentParcel, SlCommand};

use crate::avatar_menu::{SelfGroundSit, UNIMPLEMENTED};
use crate::pie_menu::{Compass, OpenPieMenu, PieAction, PieContent, PieEntry, PieMenuDef};
use crate::terrain::TerrainSurface;
use crate::ui_element::UiAction;

/// The `element` the land pie attributes its [`UiAction`]s to.
pub(crate) const LAND_MENU_ELEMENT: &str = "land-menu";

/// The land pie. See `menu_pie_land.xml`: About Land, Create, Go Here, Sit
/// Here, Mute Part. Own., Buy Pass, Edit Terrain, Buy This Land (reference
/// slots 0..7 → compass East..SouthEast).
pub(crate) static LAND_PIE: PieMenuDef = PieMenuDef {
    label: "Land",
    entries: &[
        PieEntry {
            at: Compass::East,
            content: PieContent::Action(PieAction {
                label: "About Land...",
                action: "about-land",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::NorthEast,
            content: PieContent::Action(PieAction {
                label: "Create",
                action: "build",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Go Here",
                action: "go-here",
                when: Some(UNIMPLEMENTED),
            }),
        },
        // The reference enables Sit Here whenever the pick has a valid position
        // (`Land.CanSit`); an open pie always has one here, so no condition.
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Sit Here",
                action: "sit-here",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Mute Part. Own.",
                action: "mute-particles",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthWest,
            content: PieContent::Action(PieAction {
                label: "Buy Pass",
                action: "buy-pass",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Edit Terrain",
                action: "edit-terrain",
                when: Some(UNIMPLEMENTED),
            }),
        },
        PieEntry {
            at: Compass::SouthEast,
            content: PieContent::Action(PieAction {
                label: "Buy This Land",
                action: "buy-land",
                when: Some(UNIMPLEMENTED),
            }),
        },
    ],
};

// ---------------------------------------------------------------------------
// The pick: resolving the world ray to bare terrain, for the shared resolver.
// ---------------------------------------------------------------------------

/// A world ray resolved to the terrain: what the right-click resolver compares
/// against the avatar pick (an object first-hit and a terrain first-hit are
/// mutually exclusive — same ray, same filter).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LandRayHit {
    /// The hit distance along the ray, in metres — compared against the avatar
    /// pick so the nearer target wins.
    pub(crate) distance: f32,
}

/// Resolve `ray` to the terrain, first-hit-only: `Some` exactly when the
/// nearest thing the ray strikes is a land patch ([`TerrainSurface`]), so
/// terrain behind an object or avatar body is never picked through it.
///
/// `exclude` is the HUD entity set, the same exclusion the object pick applies
/// (a HUD is screen-space and never a world pick).
pub(crate) fn pick_land(
    ray: Ray3d,
    ray_cast: &mut MeshRayCast,
    terrain: &Query<(), With<TerrainSurface>>,
    exclude: &HashSet<Entity>,
) -> Option<LandRayHit> {
    let world_filter = |entity: Entity| !exclude.contains(&entity);
    let settings = MeshRayCastSettings::default().with_filter(&world_filter);
    let (entity, hit) = ray_cast.cast_ray(ray, &settings).first().cloned()?;
    terrain.contains(entity).then_some(LandRayHit {
        distance: hit.distance,
    })
}

// ---------------------------------------------------------------------------
// The widget-facing wiring: open request → open pie → dispatch.
// ---------------------------------------------------------------------------

/// A resolved request to open the land pie at screen point `at`.
///
/// Written by the shared right-click resolver in [`crate::avatar_menu`] once a
/// right-click has resolved to bare terrain nearer than any avatar or object,
/// and consumed by [`open_land_menu`]. Unlike the object and avatar pies the
/// land pie needs no target stash yet: its one wired action (Sit Here) is
/// global. The clicked ground *position* joins the open request when the first
/// action that consumes it (Go Here, Buy Pass) goes live.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenLandMenu {
    /// Where to centre the pie, in logical pixels.
    pub(crate) at: Vec2,
}

/// The plugin wiring the land context menu into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LandMenuPlugin;

impl Plugin for LandMenuPlugin {
    /// Register the open request and the systems that turn a resolved terrain
    /// pick into an open pie and a picked slice into a command.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenLandMenu>()
            .add_systems(Update, (open_land_menu, handle_land_menu_actions).chain());
    }
}

/// Turn a resolved terrain pick into an open pie.
///
/// No conditions: the land pie's only live slice (Sit Here) is unconditional,
/// and every placeholder is gated on the never-supplied [`UNIMPLEMENTED`].
fn open_land_menu(mut requests: MessageReader<OpenLandMenu>, mut pies: MessageWriter<OpenPieMenu>) {
    for request in requests.read() {
        pies.write(OpenPieMenu {
            menu: &LAND_PIE,
            at: request.at,
            element: LAND_MENU_ELEMENT,
            conditions: Vec::new(),
        });
    }
}

/// Dispatch a picked land-menu slice to the command behind it.
///
/// Only Sit Here is wired; every other slice is a disabled placeholder that
/// never emits, so the fall-through is intentionally silent.
fn handle_land_menu_actions(
    mut actions: MessageReader<UiAction>,
    parcel: Res<SlAgentParcel>,
    mut ground_sit: ResMut<SelfGroundSit>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        if action.element != LAND_MENU_ELEMENT {
            continue;
        }
        match action.action {
            "sit-here" => {
                // The reference's `LLLandSit` stands an already-seated avatar up
                // before sitting on the ground; an object-seated avatar would
                // otherwise ignore the ground-sit control bit.
                if parcel.seated_on.is_some() {
                    commands.write(SlCommand(Command::Stand));
                }
                ground_sit.sitting = true;
                commands.write(SlCommand(Command::SitOnGround));
            }
            // Every other slice is a disabled placeholder: no behaviour yet.
            _other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LAND_PIE;
    use crate::avatar_menu::UNIMPLEMENTED;
    use crate::pie_menu::{
        Compass, PieAddress, PieConditions, ResolvedSlot, SlotOutcome, addresses, resolve_slots,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The resolved slot at `point`, or a test error naming what was missing.
    fn slot_at(
        slots: &[Option<ResolvedSlot>; crate::pie_menu::PIE_SLICES],
        point: Compass,
    ) -> Result<ResolvedSlot, TestError> {
        slots
            .get(point.slot())
            .copied()
            .flatten()
            .ok_or_else(|| format!("no slot at {}", point.name()).into())
    }

    /// **The land pie's address table, pinned.**
    ///
    /// Moving any land action to a different compass position re-teaches every
    /// user who learned this menu with their hand; this table makes that a loud
    /// diff. If a move is intended, change the table in the same reviewed
    /// commit.
    #[test]
    fn land_pie_keeps_every_address() {
        use Compass::{East, North, NorthEast, NorthWest, South, SouthEast, SouthWest, West};
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("about-land", vec![East]),
            ("build", vec![NorthEast]),
            ("go-here", vec![North]),
            ("sit-here", vec![NorthWest]),
            ("mute-particles", vec![West]),
            ("buy-pass", vec![SouthWest]),
            ("edit-terrain", vec![South]),
            ("buy-land", vec![SouthEast]),
        ];
        let actual: Vec<(&str, Vec<Compass>)> = addresses(&LAND_PIE)
            .into_iter()
            .map(|(action, PieAddress(path))| (action, path))
            .collect();
        assert_eq!(
            actual, expected,
            "a land pie action moved — if intended, bless it by editing this table"
        );
    }

    /// The land pie declares no two entries at one compass position — a silent
    /// overwrite whose winner would depend on declaration order.
    #[test]
    fn no_two_entries_share_a_position() {
        for point in Compass::ALL {
            let count = LAND_PIE
                .entries
                .iter()
                .filter(|entry| entry.at == point)
                .count();
            assert!(
                count <= 1,
                "the land pie declares {count} entries at {}",
                point.name()
            );
        }
    }

    /// In the live viewer's actual state (no conditions supplied), Sit Here is
    /// the one live slice and every placeholder keeps its slot but reads
    /// disabled — the reference menu shape present before the features are.
    #[test]
    fn sit_here_is_live_and_placeholders_are_disabled() -> Result<(), TestError> {
        let plain = resolve_slots(&LAND_PIE, &PieConditions::default());
        let sit = slot_at(&plain, Compass::NorthWest)?;
        assert_eq!(sit.outcome, SlotOutcome::Action("sit-here"));
        assert!(sit.enabled, "Sit Here must be live unconditionally");
        for (point, name) in [
            (Compass::East, "About Land..."),
            (Compass::NorthEast, "Create"),
            (Compass::North, "Go Here"),
            (Compass::West, "Mute Part. Own."),
            (Compass::SouthWest, "Buy Pass"),
            (Compass::South, "Edit Terrain"),
            (Compass::SouthEast, "Buy This Land"),
        ] {
            assert!(
                !slot_at(&plain, point)?.enabled,
                "{name} is a placeholder and must read disabled until it is wired"
            );
        }
        // The proof that the sentinel is what disables them: hold it, and they
        // light up. The live viewer never does this.
        let held = resolve_slots(&LAND_PIE, &PieConditions::new([UNIMPLEMENTED]));
        assert!(
            slot_at(&held, Compass::East)?.enabled,
            "holding the sentinel proves it is the only thing gating the placeholder"
        );
        Ok(())
    }
}
