//! The key-toggled **pipeline-status overlay**: a `bevy_ui` text node pinned to
//! the top-left corner (hidden by default, toggled with [`PIPELINE_TOGGLE_KEY`])
//! rendering the texture and mesh fetch/decode pipeline status.
//!
//! This is the P19.3 slice. The frame rate and per-frame budget the module once
//! also showed in a top-right overlay now live in the status area
//! ([`crate::status_bar`]) as a user-facing read-out, so only the developer
//! pipeline panel remains here. It renders the P19.2 [`StoreStats`] /
//! [`GateStats`] snapshots — per-stage entry counts (queued / downloading /
//! decoding / ready / failed), the in-memory footprint, the cumulative
//! disk-cache-hit and GC counters, and the admission gate's in-flight / waiting
//! figures. The rendering-fidelity phases drive these pipelines hard, so this
//! makes the LOD and priority work watchable live. Reference: Firestorm's
//! `LLTextureFetch` / `LLMeshRepository` queue stats.

use bevy::prelude::*;
use sl_client_bevy::{GateStats, StoreStats};

use crate::meshes::MeshManager;
use crate::textures::TextureManager;
use crate::ui_font::UiFont;

/// The overlay font size, in logical pixels.
const DIAG_FONT_SIZE: f32 = 15.0;

/// The inset, in logical pixels, of the pipeline overlay from the top-left corner.
const DIAG_INSET: f32 = 10.0;

/// The key that toggles the pipeline-status overlay on and off.
const PIPELINE_TOGGLE_KEY: KeyCode = KeyCode::F3;

/// Whether the pipeline-status overlay (P19.3) is currently shown. Toggled by
/// [`PIPELINE_TOGGLE_KEY`]; hidden by default so it stays out of the way until
/// the fetch/decode pipeline is being watched.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct PipelineOverlayVisible(pub(crate) bool);

impl PipelineOverlayVisible {
    /// The initial visibility, seeded from the `SL_VIEWER_PIPELINE_OVERLAY`
    /// environment variable so the offline screenshot harness (which cannot
    /// press [`PIPELINE_TOGGLE_KEY`]) can capture the panel: set to start shown,
    /// unset to start hidden (the interactive default). The `F3` key still
    /// toggles it either way.
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
/// [`PIPELINE_TOGGLE_KEY`] — and is rewritten each frame it is visible from the
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
            top: Val::Px(DIAG_INSET),
            left: Val::Px(DIAG_INSET),
            ..default()
        },
        Visibility::Hidden,
        PipelineStatusText,
    ));
}

/// Toggle the pipeline-status overlay when [`PIPELINE_TOGGLE_KEY`] is pressed.
pub(crate) fn toggle_pipeline_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<PipelineOverlayVisible>,
) {
    if keyboard.just_pressed(PIPELINE_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the pipeline-status node's visibility from [`PipelineOverlayVisible`],
/// and — while it is shown — rewrite it from the live texture / mesh store and
/// gate snapshots (P19.2). The stats are only sampled when the panel is visible,
/// so the hidden default costs nothing beyond the toggle check.
pub(crate) fn update_pipeline_overlay(
    visible: Res<PipelineOverlayVisible>,
    textures: Res<TextureManager>,
    meshes: Res<MeshManager>,
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
    *text = Text::new(format_pipeline(
        textures.stats(),
        textures.gate_stats(),
        meshes.stats(),
        meshes.gate_stats(),
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
fn format_store_block(label: &str, stats: StoreStats, gate: GateStats) -> String {
    format!(
        "{label:<5} queued {}  dl {}  dec {}  ready {}  fail {}\n\
         {:<5} mem {} ({})  cached {}  gc {}  gate {}/{} wait {}",
        stats.queued,
        stats.downloading,
        stats.decoding,
        stats.ready,
        stats.failed,
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

/// Format the whole pipeline-status panel: a header, then one two-line block per
/// pipeline (texture, then mesh).
fn format_pipeline(
    tex: StoreStats,
    tex_gate: GateStats,
    mesh: StoreStats,
    mesh_gate: GateStats,
) -> String {
    format!(
        "PIPELINE  (F3)\n{}\n{}",
        format_store_block("tex", tex, tex_gate),
        format_store_block("mesh", mesh, mesh_gate),
    )
}

#[cfg(test)]
mod tests {
    use super::{GateStats, StoreStats, format_bytes, format_pipeline, format_store_block};
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
            format_store_block("tex", stats, gate),
            "tex   queued 3  dl 2  dec 1  ready 840  fail 0\n      \
             mem 840 (128.0 MiB)  cached 512  gc 4  gate 6/8 wait 0"
        );
    }

    /// The full panel carries the header and both store blocks in order.
    #[test]
    fn pipeline_panel_has_header_and_both_blocks() {
        let panel = format_pipeline(
            StoreStats::default(),
            GateStats::default(),
            StoreStats::default(),
            GateStats::default(),
        );
        let mut lines = panel.lines();
        assert_eq!(lines.next(), Some("PIPELINE  (F3)"));
        // Header, then two lines per block for two blocks.
        assert_eq!(panel.lines().count(), 5);
        assert!(panel.contains("tex   queued 0"));
        assert!(panel.contains("mesh  queued 0"));
    }
}
