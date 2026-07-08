//! On-screen diagnostics overlay: a `bevy_ui` text node pinned to the top-right
//! corner that shows the rendering instruments — frames-per-second, per-frame
//! milliseconds, the total ECS entity count, and the number of drawn meshes.
//!
//! This is the Phase 19 slice — the first of the rendering-fidelity phases,
//! which drive the fetch / decode pipeline much harder, so this panel gives us
//! the instruments to watch the frame budget while that work lands. It reuses
//! the Phase 11 chat-overlay pattern from [`chat`](crate::chat): one persistent
//! absolute-positioned [`Text`] node whose string is rebuilt each frame.
//!
//! The three frame diagnostics come from Bevy's
//! [`FrameTimeDiagnosticsPlugin`] (FPS / frame-time) and
//! [`EntityCountDiagnosticsPlugin`] (entity count), both smoothed over their
//! rolling history. The "draws" figure is the count of [`Mesh3d`] instances in
//! the main world — an approximation of the per-frame draw calls (one opaque
//! mesh is broadly one draw), the same coarse gauge the reference viewer's
//! statistics floater surfaces. Reference: Firestorm `LLViewerStats` /
//! `LLFastTimerView` / `LLPerfStats`.

use bevy::diagnostic::{
    Diagnostic, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;

/// The overlay font size, in logical pixels.
const DIAG_FONT_SIZE: f32 = 15.0;

/// The inset, in logical pixels, of the overlay from the top-right corner.
const DIAG_INSET: f32 = 10.0;

/// A marker component tagging the single diagnostics text node, so the update
/// system can find and rewrite it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DiagnosticsOverlayText;

/// Startup system: spawn the persistent diagnostics text node, pinned to the
/// top-right corner (clear of the bottom-left chat overlay). Its lines are
/// right-justified so the block stays flush against the right edge as the
/// numbers change width. It starts empty and is rewritten each frame from the
/// live diagnostics.
pub(crate) fn setup_diagnostics_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: FontSize::Px(DIAG_FONT_SIZE),
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::default().with_justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(DIAG_INSET),
            right: Val::Px(DIAG_INSET),
            ..default()
        },
        DiagnosticsOverlayText,
    ));
}

/// Format one smoothed diagnostic value with the given number of decimal places,
/// falling back to a placeholder while the diagnostic has not yet been measured.
fn format_value(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| "--".to_owned(), |value| format!("{value:.decimals$}"))
}

/// Format the diagnostics block: FPS and frame-time on the first line, entity
/// and draw counts on the second.
fn format_diagnostics(
    fps: Option<f64>,
    frame_ms: Option<f64>,
    entities: Option<f64>,
    draws: usize,
) -> String {
    format!(
        "FPS {}  ({} ms)\nentities {}  draws {draws}",
        format_value(fps, 0),
        format_value(frame_ms, 1),
        format_value(entities, 0),
    )
}

/// Rewrite the diagnostics overlay each frame from the live FPS / frame-time /
/// entity-count diagnostics and the current [`Mesh3d`] instance count.
pub(crate) fn update_diagnostics_overlay(
    diagnostics: Res<DiagnosticsStore>,
    meshes: Query<(), With<Mesh3d>>,
    mut texts: Query<&mut Text, With<DiagnosticsOverlayText>>,
) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(Diagnostic::smoothed);
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(Diagnostic::smoothed);
    let entities = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(Diagnostic::smoothed);
    let draws = meshes.iter().count();
    *text = Text::new(format_diagnostics(fps, frame_ms, entities, draws));
}

#[cfg(test)]
mod tests {
    use super::{format_diagnostics, format_value};
    use pretty_assertions::assert_eq;

    /// A measured value is formatted at the requested precision; a missing one
    /// renders as the `--` placeholder.
    #[test]
    fn value_formatting_rounds_and_falls_back() {
        assert_eq!(format_value(Some(59.97), 0), "60");
        assert_eq!(format_value(Some(16.64), 1), "16.6");
        assert_eq!(format_value(None, 0), "--");
        assert_eq!(format_value(None, 1), "--");
    }

    /// The full block places FPS / frame-time on the first line and the entity /
    /// draw counts on the second, and stays intact when a diagnostic is missing.
    #[test]
    fn block_has_two_lines() {
        assert_eq!(
            format_diagnostics(Some(60.0), Some(16.6), Some(1234.0), 567),
            "FPS 60  (16.6 ms)\nentities 1234  draws 567"
        );
        assert_eq!(
            format_diagnostics(None, None, None, 0),
            "FPS --  (-- ms)\nentities --  draws 0"
        );
    }
}
