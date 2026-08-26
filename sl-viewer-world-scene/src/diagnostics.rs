//! The key-toggled **pipeline-status overlay**: a `bevy_ui` text node pinned to
//! the top-left corner (hidden by default, toggled with `PIPELINE_TOGGLE_KEY`)
//! rendering the asset fetch/decode pipeline status. The texture and mesh stores
//! get a full two-line block each; the asset stores added since — animations,
//! settings/environment, sounds, wearable bake-inputs, and glTF materials — get
//! one condensed line each, so "nothing left to load" on F3 accounts for every
//! pipeline rather than just the first two.
//!
//! This is the P19.3 slice. The frame rate and per-frame budget the module once
//! also showed in a top-right overlay now live in the status area
//! (`crate::status_bar`) as a user-facing read-out, so only the developer
//! pipeline panel remains here. It renders the P19.2 [`StoreStats`] /
//! [`GateStats`] snapshots — per-stage entry counts (queued / downloading /
//! decoding / ready / failed), the in-memory footprint, the cumulative
//! disk-cache-hit and GC counters, and the admission gate's in-flight / waiting
//! figures. The rendering-fidelity phases drive these pipelines hard, so this
//! makes the LOD and priority work watchable live. Reference: Firestorm's
//! `LLTextureFetch` / `LLMeshRepository` queue stats.

use bevy::prelude::*;
use sl_client_bevy::{GateStats, StoreStats};

use crate::animations::AnimationManager;
use crate::bake_inputs::WearableAssetManager;
use crate::environment_assets::EnvironmentAssetManager;
use crate::geometry_cache::{GeometryCache, GeometryCacheStats};
use crate::material_cache::{MaterialCache, MaterialCacheStats};
use crate::materials::MaterialManager;
use crate::meshes::MeshManager;
use crate::sound_cache::SoundCache;
use crate::textures::TextureManager;
use crate::ui_font::UiFont;

/// The pipeline-status overlay's own scheduling (P19.3): the `F3` panel that
/// reports the texture / mesh / animation stores' live fetch and decode state.
#[derive(Debug, Default)]
pub struct PipelineOverlayPlugin;

impl Plugin for PipelineOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PipelineOverlayVisible::from_env())
            .add_systems(Startup, setup_pipeline_overlay)
            .add_systems(
                Update,
                (
                    toggle_pipeline_overlay,
                    update_pipeline_overlay
                        .run_if(pipeline_overlay_active)
                        .after(toggle_pipeline_overlay),
                ),
            );
    }
}

/// The overlay font size, in logical pixels.
const DIAG_FONT_SIZE: f32 = 15.0;

/// The inset, in logical pixels, of the pipeline overlay from the left edge.
const DIAG_INSET: f32 = 10.0;

/// The inset, in logical pixels, of the pipeline overlay from the top of the
/// window — larger than [`DIAG_INSET`] so the panel starts *below* the
/// full-width top menu/status bar (which renders above floaters at `TOP_BAR_Z`
/// and was covering the panel's first lines).
const DIAG_TOP_INSET: f32 = 42.0;

/// The key that toggles the pipeline-status overlay on and off.
const PIPELINE_TOGGLE_KEY: KeyCode = KeyCode::F3;

/// Whether the pipeline-status overlay (P19.3) is currently shown. Toggled by
/// `PIPELINE_TOGGLE_KEY`; hidden by default so it stays out of the way until
/// the fetch/decode pipeline is being watched.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct PipelineOverlayVisible(pub(crate) bool);

impl PipelineOverlayVisible {
    /// The initial visibility, seeded from the `SL_VIEWER_PIPELINE_OVERLAY`
    /// environment variable so the offline screenshot harness (which cannot
    /// press `PIPELINE_TOGGLE_KEY`) can capture the panel: set to start shown,
    /// unset to start hidden (the interactive default). The `F3` key still
    /// toggles it either way.
    #[must_use]
    pub(crate) fn from_env() -> Self {
        Self(std::env::var_os("SL_VIEWER_PIPELINE_OVERLAY").is_some())
    }
}

/// A marker component tagging the single pipeline-status text node, so the
/// update system can find and rewrite it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PipelineStatusText;

/// Startup system: spawn the persistent pipeline-status text node, pinned to the
/// top-left corner (clear of the top-right frame overlay and the bottom-left
/// chat overlay). It starts [`Visibility::Hidden`] — the panel is opt-in via
/// `PIPELINE_TOGGLE_KEY` — and is rewritten each frame it is visible from the
/// live store snapshots.
pub(crate) fn setup_pipeline_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        // Monospace, as for the frame overlay above: the panel is tabular
        // per-pipeline counters.
        UiFont::Mono.at(DIAG_FONT_SIZE),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(DIAG_TOP_INSET),
            left: Val::Px(DIAG_INSET),
            ..default()
        },
        Visibility::Hidden,
        PipelineStatusText,
    ));
}

/// Toggle the pipeline-status overlay when `PIPELINE_TOGGLE_KEY` is pressed.
pub(crate) fn toggle_pipeline_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<PipelineOverlayVisible>,
) {
    if keyboard.just_pressed(PIPELINE_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Run condition for [`update_pipeline_overlay`]: the overlay is shown, or its
/// visibility just flipped (the changed arm runs the hide write on toggle-off
/// and the initial sync) — a hidden overlay costs no dispatch at all.
#[must_use]
pub(crate) fn pipeline_overlay_active(visible: Res<PipelineOverlayVisible>) -> bool {
    visible.0 || visible.is_changed()
}

/// Drive the pipeline-status node's visibility from [`PipelineOverlayVisible`],
/// and — while it is shown — rewrite it from the live texture / mesh store and
/// gate snapshots (P19.2). The stats are only sampled when the panel is visible,
/// so the hidden default costs nothing beyond the toggle check.
#[expect(
    clippy::too_many_arguments,
    reason = "one asset-store resource per pipeline sampled into the overlay"
)]
pub(crate) fn update_pipeline_overlay(
    visible: Res<PipelineOverlayVisible>,
    textures: Res<TextureManager>,
    meshes: Res<MeshManager>,
    animations: Res<AnimationManager>,
    environment: Res<EnvironmentAssetManager>,
    sounds: Res<SoundCache>,
    wearables: Res<WearableAssetManager>,
    gltf_materials: Res<MaterialManager>,
    geometry: Res<GeometryCache>,
    material: Res<MaterialCache>,
    mut panels: Query<(&mut Text, &mut Visibility), With<PipelineStatusText>>,
) {
    let Ok((mut text, mut visibility)) = panels.single_mut() else {
        return;
    };
    if !visible.0 {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    if *visibility != Visibility::Visible {
        *visibility = Visibility::Visible;
    }
    // The asset managers added since the overlay was built get one condensed
    // line each (a full two-line block per store would make the debug panel too
    // tall). `gmat` is the glTF-material *asset* store, distinct from the interned
    // `FaceMaterial` `mat` cache line below.
    let condensed = [
        (
            "anim",
            animations.stats(),
            animations.gate_stats(),
            animations.deferred_count(),
        ),
        (
            "env",
            environment.stats(),
            environment.gate_stats(),
            environment.deferred_count(),
        ),
        (
            "sound",
            sounds.stats(),
            sounds.gate_stats(),
            sounds.deferred_count(),
        ),
        (
            "wear",
            wearables.stats(),
            wearables.gate_stats(),
            wearables.deferred_count(),
        ),
        (
            "gmat",
            gltf_materials.stats(),
            gltf_materials.gate_stats(),
            gltf_materials.deferred_count(),
        ),
    ];
    *text = Text::new(format_pipeline(
        textures.stats(),
        textures.gate_stats(),
        textures.deferred_count(),
        meshes.stats(),
        meshes.gate_stats(),
        meshes.deferred_count(),
        &condensed,
        geometry.stats(),
        material.stats(),
    ));
}

/// Render a byte count as mebibytes with one decimal place, using integer math
/// (the workspace denies `as` casts, so no float conversion).
fn format_bytes(bytes: u64) -> String {
    // Tenths of a MiB, rounded down; `saturating_mul` guards the (unreachable in
    // practice) overflow of a multi-exbibyte footprint.
    let tenths = bytes.saturating_mul(10) / (1024 * 1024);
    format!("{}.{} MiB", tenths / 10, tenths % 10)
}

/// Format one store's two-line block: the per-stage entry counts on the first
/// line, then the in-memory footprint, cumulative cache-hit / GC counters, and
/// the admission gate's in-flight / capacity / waiting figures on the second.
fn format_store_block(label: &str, stats: StoreStats, gate: GateStats, deferred: usize) -> String {
    format!(
        "{label:<5} queued {}  dl {}  dec {}  ready {}  fail {}  defer {}\n\
         {:<5} mem {} ({})  cached {}  gc {}  gate {}/{} wait {}",
        stats.queued,
        stats.downloading,
        stats.decoding,
        stats.ready,
        stats.failed,
        deferred,
        "",
        stats.in_memory,
        format_bytes(stats.bytes),
        stats.cache_hits,
        stats.collected,
        gate.in_flight,
        gate.capacity,
        gate.waiting,
    )
}

/// Format one store's **condensed** single line — the compact form used for the
/// asset stores added since the overlay was built, so five more pipelines do not
/// each cost the two-line [`format_store_block`] and push the debug panel off the
/// screen. Carries the per-stage counts, the deferred (parked / retrying) count,
/// the in-memory footprint, and the admission gate's in-flight / capacity /
/// waiting figures on one line.
fn format_store_line(label: &str, stats: StoreStats, gate: GateStats, deferred: usize) -> String {
    format!(
        "{label:<5} q{} dl{} dec{} rdy{} f{} def{}  mem {} ({})  gate {}/{} w{}",
        stats.queued,
        stats.downloading,
        stats.decoding,
        stats.ready,
        stats.failed,
        deferred,
        stats.in_memory,
        format_bytes(stats.bytes),
        gate.in_flight,
        gate.capacity,
        gate.waiting,
    )
}

/// Format the cross-instance geometry cache's one-line block: how many distinct
/// geometries are cached and the cumulative spawn outcomes (full hits that
/// skipped tessellation, partial hits that revived some faces, misses), then
/// the rigged-submesh slots (entries and per-submesh revive hits / misses).
fn format_geometry_block(stats: GeometryCacheStats) -> String {
    format!(
        "geom  entries {}  hit {}  partial {}  miss {}  rigged {} hit {} miss {}",
        stats.entries,
        stats.hits,
        stats.partial_hits,
        stats.misses,
        stats.rigged_entries,
        stats.rigged_hits,
        stats.rigged_misses,
    )
}

/// Format the cross-instance material cache's one-line block: how many distinct
/// face materials are cached and the cumulative face outcomes (hits that shared
/// an existing material, misses that composed and recorded a fresh one, faces
/// excluded from interning).
fn format_material_block(stats: MaterialCacheStats) -> String {
    format!(
        "mat   entries {}  hit {}  miss {}  excl {}",
        stats.entries, stats.hits, stats.misses, stats.excluded,
    )
}

/// Format the whole pipeline-status panel: a header, then the two-line
/// texture and mesh blocks, then one condensed [`format_store_line`] per
/// later-added asset store (`condensed`, in display order), then the
/// geometry-cache and material-cache lines.
#[expect(
    clippy::too_many_arguments,
    reason = "the two detailed stores plus the condensed slice and two caches rendered in the overlay"
)]
fn format_pipeline(
    tex: StoreStats,
    tex_gate: GateStats,
    tex_deferred: usize,
    mesh: StoreStats,
    mesh_gate: GateStats,
    mesh_deferred: usize,
    condensed: &[(&str, StoreStats, GateStats, usize)],
    geometry: GeometryCacheStats,
    material: MaterialCacheStats,
) -> String {
    let mut panel = format!(
        "PIPELINE  (F3)\n{}\n{}\n",
        format_store_block("tex", tex, tex_gate, tex_deferred),
        format_store_block("mesh", mesh, mesh_gate, mesh_deferred),
    );
    for (label, stats, gate, deferred) in condensed {
        panel.push_str(&format_store_line(label, *stats, *gate, *deferred));
        panel.push('\n');
    }
    panel.push_str(&format_geometry_block(geometry));
    panel.push('\n');
    panel.push_str(&format_material_block(material));
    panel
}

#[cfg(test)]
mod tests {
    use super::{
        GateStats, GeometryCacheStats, MaterialCacheStats, StoreStats, format_bytes,
        format_geometry_block, format_material_block, format_pipeline, format_store_block,
        format_store_line,
    };
    use pretty_assertions::assert_eq;

    /// Bytes render as MiB with one decimal via integer math, flooring the
    /// fraction and handling the zero case.
    #[test]
    fn bytes_render_as_mib() {
        assert_eq!(format_bytes(0), "0.0 MiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        // 1.5 MiB exactly.
        assert_eq!(format_bytes(1024 * 1024 * 3 / 2), "1.5 MiB");
        // 128 MiB.
        assert_eq!(format_bytes(128 * 1024 * 1024), "128.0 MiB");
    }

    /// One store block places the per-stage counts on the first line and the
    /// footprint / counters / gate on the second, left-padded under the label.
    #[test]
    fn store_block_has_two_lines() {
        let stats = StoreStats {
            queued: 3,
            downloading: 2,
            decoding: 1,
            ready: 840,
            failed: 0,
            in_memory: 840,
            bytes: 128 * 1024 * 1024,
            cache_hits: 512,
            collected: 4,
            ..StoreStats::default()
        };
        let gate = GateStats {
            capacity: 8,
            in_flight: 6,
            waiting: 0,
        };
        assert_eq!(
            format_store_block("tex", stats, gate, 7),
            "tex   queued 3  dl 2  dec 1  ready 840  fail 0  defer 7\n      \
             mem 840 (128.0 MiB)  cached 512  gc 4  gate 6/8 wait 0"
        );
    }

    /// The condensed one-line store form places every stage count, the deferred
    /// count, the footprint, and the gate on a single line, left-padded under the
    /// label — the compact form for the later-added asset stores.
    #[test]
    fn store_line_is_one_line() {
        let stats = StoreStats {
            queued: 3,
            downloading: 2,
            decoding: 1,
            ready: 40,
            failed: 0,
            in_memory: 40,
            bytes: 2 * 1024 * 1024,
            ..StoreStats::default()
        };
        let gate = GateStats {
            capacity: 16,
            in_flight: 5,
            waiting: 1,
        };
        let line = format_store_line("anim", stats, gate, 4);
        assert_eq!(
            line,
            "anim  q3 dl2 dec1 rdy40 f0 def4  mem 40 (2.0 MiB)  gate 5/16 w1"
        );
        // A single line: no embedded newline.
        assert_eq!(line.lines().count(), 1);
    }

    /// The geometry-cache block renders its entry count, the three spawn
    /// outcome counters, and the rigged-slot counters on one line.
    #[test]
    fn geometry_block_is_one_line() {
        let stats = GeometryCacheStats {
            entries: 12,
            hits: 340,
            partial_hits: 5,
            misses: 48,
            rigged_entries: 3,
            rigged_hits: 21,
            rigged_misses: 7,
        };
        assert_eq!(
            format_geometry_block(stats),
            "geom  entries 12  hit 340  partial 5  miss 48  rigged 3 hit 21 miss 7"
        );
    }

    /// The material-cache block renders its entry count and the three face
    /// outcome counters on one line.
    #[test]
    fn material_block_is_one_line() {
        let stats = MaterialCacheStats {
            entries: 9,
            hits: 210,
            misses: 33,
            excluded: 17,
        };
        assert_eq!(
            format_material_block(stats),
            "mat   entries 9  hit 210  miss 33  excl 17"
        );
    }

    /// The full panel carries the header, the two detailed store blocks, one
    /// condensed line per later-added store, and the geometry-cache and
    /// material-cache lines in order.
    #[test]
    fn pipeline_panel_has_header_and_both_blocks() {
        let condensed = [
            ("anim", StoreStats::default(), GateStats::default(), 0),
            ("env", StoreStats::default(), GateStats::default(), 0),
            ("sound", StoreStats::default(), GateStats::default(), 0),
            ("wear", StoreStats::default(), GateStats::default(), 0),
            ("gmat", StoreStats::default(), GateStats::default(), 0),
        ];
        let panel = format_pipeline(
            StoreStats::default(),
            GateStats::default(),
            0,
            StoreStats::default(),
            GateStats::default(),
            0,
            &condensed,
            GeometryCacheStats::default(),
            MaterialCacheStats::default(),
        );
        let mut lines = panel.lines();
        assert_eq!(lines.next(), Some("PIPELINE  (F3)"));
        // Header, two lines per detailed block for two blocks (4), one condensed
        // line per later-added store (5), then the geometry-cache and
        // material-cache lines (2): 1 + 4 + 5 + 2 = 12.
        assert_eq!(panel.lines().count(), 12);
        assert!(panel.contains("tex   queued 0"));
        assert!(panel.contains("mesh  queued 0"));
        assert!(panel.contains("anim  q0 dl0 dec0 rdy0 f0 def0"));
        assert!(panel.contains("env   q0"));
        assert!(panel.contains("sound q0"));
        assert!(panel.contains("wear  q0"));
        assert!(panel.contains("gmat  q0"));
        assert!(panel.contains("geom  entries 0"));
        assert!(panel.contains("mat   entries 0"));
    }
}
