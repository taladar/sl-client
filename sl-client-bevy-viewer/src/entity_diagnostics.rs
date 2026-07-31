//! Viewer entity-population diagnostics streamed to Tracy, breaking Bevy's
//! single `entity_count` (main-world total) down by kind so a capture shows
//! *what* the tens of thousands of entities are — the deciding factor for which
//! entity-reduction lever is worth pulling.
//!
//! Bevy's [`EntityCountDiagnosticsPlugin`](bevy::diagnostic::EntityCountDiagnosticsPlugin)
//! counts only the **main** app world and only as one lump. This adds:
//!
//! * per-kind main-world counts — UI nodes, in-world object roots (prims), their
//!   tessellated faces, avatars, and everything carrying a `Mesh3d` (the
//!   "rendered in 3D" set); "rendered at all" is `entity/ui + entity/mesh3d` and
//!   "other ECS" (skeleton joints, hierarchy anchors, parcels, data-only
//!   entities) is `entity_count` minus that;
//! * the **render world** entity total — a separate sub-world Bevy's diagnostic
//!   never sees, bridged out of the [`RenderApp`] through a shared atomic.
//!
//! Each is an ordinary [`bevy::diagnostic::Diagnostic`], so [`crate::tracy_plots`]
//! streams them as plots with no extra wiring. Compiled only under
//! `profile-tracy`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::diagnostic::{Diagnostic, DiagnosticPath, Diagnostics, RegisterDiagnostic};
use bevy::ecs::entity::Entities;
use bevy::prelude::*;
use bevy::render::{Render, RenderApp};

use crate::avatars::AvatarAnchor;
use crate::objects::{PrimFaceEntity, SceneObject};

/// UI-node entities (`bevy_ui`).
const ENTITY_UI: DiagnosticPath = DiagnosticPath::const_new("entity/ui");
/// In-world object roots — one per prim/linkset-child ([`SceneObject`]).
const ENTITY_OBJECTS: DiagnosticPath = DiagnosticPath::const_new("entity/objects");
/// Tessellated prim-face child entities ([`PrimFaceEntity`]) — usually the bulk.
const ENTITY_FACES: DiagnosticPath = DiagnosticPath::const_new("entity/faces");
/// Avatars — one anchor entity each ([`AvatarAnchor`]).
const ENTITY_AVATARS: DiagnosticPath = DiagnosticPath::const_new("entity/avatars");
/// Everything carrying a `Mesh3d` — the "rendered in 3D" main-world set.
const ENTITY_MESH3D: DiagnosticPath = DiagnosticPath::const_new("entity/mesh3d");
/// The render world's own entity total (bridged out of the [`RenderApp`]).
const ENTITY_RENDER_WORLD: DiagnosticPath = DiagnosticPath::const_new("entity/render_world");

/// A count converted for the diagnostic store.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a viewer never holds enough entities for the f64 mantissa to lose a count"
)]
fn count_f64(n: usize) -> f64 {
    n as f64
}

/// Shared cell the [`RenderApp`] writes its entity count into and the main world
/// reads back — the render world extracts *from* the main world, so there is no
/// built-in channel the other way; a relaxed atomic is enough for a per-frame
/// telemetry value (a one-frame lag on a plot is immaterial).
#[derive(Resource, Clone)]
struct RenderEntityCount(Arc<AtomicU32>);

/// Measure each main-world entity kind once per frame.
fn measure_entity_breakdown(
    mut diagnostics: Diagnostics,
    ui: Query<(), With<Node>>,
    objects: Query<(), With<SceneObject>>,
    faces: Query<(), With<PrimFaceEntity>>,
    avatars: Query<(), With<AvatarAnchor>>,
    meshes: Query<(), With<Mesh3d>>,
) {
    let n_ui = ui.iter().count();
    let n_objects = objects.iter().count();
    let n_faces = faces.iter().count();
    let n_avatars = avatars.iter().count();
    let n_meshes = meshes.iter().count();
    diagnostics.add_measurement(&ENTITY_UI, || count_f64(n_ui));
    diagnostics.add_measurement(&ENTITY_OBJECTS, || count_f64(n_objects));
    diagnostics.add_measurement(&ENTITY_FACES, || count_f64(n_faces));
    diagnostics.add_measurement(&ENTITY_AVATARS, || count_f64(n_avatars));
    diagnostics.add_measurement(&ENTITY_MESH3D, || count_f64(n_meshes));
}

/// Publish the render world's last-recorded entity count as a main-world
/// diagnostic (reads the atomic the [`RenderApp`] system wrote).
fn publish_render_entity_count(mut diagnostics: Diagnostics, shared: Res<RenderEntityCount>) {
    let n = shared.0.load(Ordering::Relaxed);
    diagnostics.add_measurement(&ENTITY_RENDER_WORLD, || f64::from(n));
}

/// Record the render world's own entity count into the shared cell (runs in the
/// [`RenderApp`], so `Entities` is the render world's).
fn record_render_entity_count(entities: &Entities, shared: Res<RenderEntityCount>) {
    shared.0.store(entities.count_spawned(), Ordering::Relaxed);
}

/// Registers the per-kind main-world counts and the bridged render-world count.
///
/// Added under `profile-tracy` (see [`crate::tracy_plots`]). Main-world
/// measurement runs in [`Update`], before the `Last` streaming system samples
/// it; the render-world wiring is done in [`Plugin::finish`] so the
/// [`RenderApp`] sub-app is guaranteed to exist.
pub(crate) struct EntityDiagnosticsPlugin;

impl Plugin for EntityDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.register_diagnostic(Diagnostic::new(ENTITY_UI).with_suffix(" ui"))
            .register_diagnostic(Diagnostic::new(ENTITY_OBJECTS).with_suffix(" objects"))
            .register_diagnostic(Diagnostic::new(ENTITY_FACES).with_suffix(" faces"))
            .register_diagnostic(Diagnostic::new(ENTITY_AVATARS).with_suffix(" avatars"))
            .register_diagnostic(Diagnostic::new(ENTITY_MESH3D).with_suffix(" meshes"))
            .register_diagnostic(
                Diagnostic::new(ENTITY_RENDER_WORLD).with_suffix(" render entities"),
            )
            .insert_resource(RenderEntityCount(Arc::new(AtomicU32::new(0))))
            .add_systems(
                Update,
                (measure_entity_breakdown, publish_render_entity_count),
            );
    }

    fn finish(&self, app: &mut App) {
        let shared = app.world().resource::<RenderEntityCount>().clone();
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(shared)
                .add_systems(Render, record_render_entity_count);
        }
    }
}
