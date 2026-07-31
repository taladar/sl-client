//! Feed the viewer's live telemetry into the Tracy profiler beyond the CPU
//! zones the `tracing` bridge already streams (see [`crate::init_tracing`]).
//!
//! Two things Bevy's `bevy/trace_tracy` integration does *not* wire up on its
//! own, both added here and compiled only under the `profile-tracy` feature:
//!
//! * **Diagnostics as plots.** Every enabled [`bevy::diagnostic::Diagnostic`]
//!   (the `FrameTimeDiagnosticsPlugin` FPS / frame-time / frame-count set plus
//!   the `EntityCountDiagnosticsPlugin` live entity count, and anything else
//!   registered later) is pushed to Tracy each frame with
//!   `Client::plot`, so it appears as a graphed line in the profiler timeline
//!   alongside the zones — the same numbers the status bar and the `F3` overlay
//!   show, but plotted over time. `tracing-tracy` has no path for this; it only
//!   forwards spans and events.
//!
//! * **A physics secondary frame mark.** The single primary frame mark Bevy
//!   emits (`tracy.frame_mark`, once per rendered frame) measures only the
//!   render cadence. The simulation runs on a *separate* fixed-timestep clock
//!   ([`bevy::time::Time<bevy::time::Fixed>`] pinned to `SL_PHYSICS_HZ` in
//!   [`crate::physics`]), which ticks zero, one, or several times per rendered
//!   frame. Marking a Tracy *secondary* (named) frame once per fixed step gives
//!   that loop its own frame-time graph and histogram in the profiler, so its
//!   cadence and jitter can be read independently of the frame rate.
//!
//! Every emit is guarded by [`Client::running`]: in the on-demand configuration
//! the client exists for the whole process but collects nothing until a profiler
//! connects, and the guard also means the systems are harmless no-ops should the
//! plugin ever run without [`crate::init_tracing`] having started a client.

use std::collections::HashMap;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use tracing_tracy::client::{Client, FrameName, PlotConfiguration, PlotFormat, PlotName};

/// Interned Tracy name handles for this process.
///
/// Both [`PlotName`] and [`FrameName`] wrap a leaked `&'static str` (Tracy keys
/// plots and frames by the string *pointer*, so the pointer must be stable for
/// the lifetime of the plot). `new_leak` leaks on every call, so each name is
/// constructed exactly once — plot names lazily as diagnostics are first seen,
/// the physics frame name up front — and reused thereafter.
#[derive(Resource)]
struct TracyNames {
    /// Plot-name handle per diagnostic, keyed by the diagnostic's path string.
    plots: HashMap<String, PlotName>,
    /// The leaked name of the physics secondary frame, emitted every fixed step.
    physics_frame: FrameName,
}

impl Default for TracyNames {
    fn default() -> Self {
        Self {
            plots: HashMap::new(),
            physics_frame: FrameName::new_leak("physics".to_owned()),
        }
    }
}

/// Guess how Tracy should display a diagnostic from its path.
///
/// Diagnostics carry no unit metadata, so the format is inferred from the name:
/// anything that reads as a fraction of a whole (`*_usage`, `percent`, `%` —
/// Bevy's `SystemInformationDiagnosticsPlugin` reports CPU *and* memory usage as
/// percentages) is shown as a percentage; explicit byte counts as a memory
/// size; everything else (FPS, frame time, frame count, entity counts) as a
/// plain number. The check order matters: `mem_usage` is a percentage, not a
/// byte count, so the percentage test runs first.
fn plot_format_for(path: &str) -> PlotFormat {
    let lower = path.to_ascii_lowercase();
    if lower.contains("usage") || lower.contains("percent") || lower.contains('%') {
        PlotFormat::Percentage
    } else if lower.contains("byte") || lower.contains("memory") {
        PlotFormat::Memory
    } else {
        PlotFormat::Number
    }
}

/// Push every enabled diagnostic's current value to Tracy as a plot point.
///
/// Runs in [`Last`] so the frame's diagnostics have already been measured. The
/// smoothed value is preferred (it matches what the on-screen read-outs show);
/// counters and other un-smoothed diagnostics fall back to their raw value. A
/// newly-seen diagnostic has its plot name interned and its display format
/// configured once before the first point is plotted.
fn stream_diagnostics_to_tracy(diagnostics: Res<DiagnosticsStore>, mut names: ResMut<TracyNames>) {
    // No client running (never started, or on-demand with no profiler attached):
    // nothing to record this frame.
    let Some(client) = Client::running() else {
        return;
    };
    for diagnostic in diagnostics.iter() {
        // `iter` yields disabled diagnostics too; skip them so a toggled-off
        // instrument does not draw a stale flat line in the profiler.
        if !diagnostic.is_enabled {
            continue;
        }
        let Some(value) = diagnostic.smoothed().or_else(|| diagnostic.value()) else {
            continue;
        };
        let path = diagnostic.path().as_str();
        let plot = if let Some(plot) = names.plots.get(path) {
            *plot
        } else {
            let plot = PlotName::new_leak(path.to_owned());
            // Configure the fresh plot once, before its first point, so Tracy
            // formats the axis correctly from the very first sample.
            client.plot_config(
                plot,
                PlotConfiguration::default().format(plot_format_for(path)),
            );
            names.plots.insert(path.to_owned(), plot);
            plot
        };
        client.plot(plot, value);
    }
}

/// Emit the physics secondary frame mark, once per fixed-timestep step.
///
/// Scheduled in [`FixedLast`], which runs exactly once per fixed step after the
/// simulation has advanced (avian steps in `FixedPostUpdate`), so each mark
/// closes one physics frame in Tracy's named-frame timeline.
fn mark_physics_frame(names: Res<TracyNames>) {
    if let Some(client) = Client::running() {
        client.secondary_frame_mark(names.physics_frame);
    }
}

/// Streams the app's diagnostics to Tracy as plots and marks the fixed-timestep
/// physics loop as a Tracy secondary frame.
///
/// Added only under the `profile-tracy` feature (this whole module compiles only
/// then). Depends on nothing beyond a `DiagnosticsStore` — present via Bevy's
/// `DiagnosticsPlugin`, pulled in by `DefaultPlugins` — so it is safe to add
/// last, after the diagnostic sources are registered.
pub(crate) struct TracyProfilingPlugin;

impl Plugin for TracyProfilingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TracyNames>()
            .add_systems(Last, stream_diagnostics_to_tracy)
            .add_systems(FixedLast, mark_physics_frame);
    }
}

#[cfg(test)]
mod tests {
    use super::plot_format_for;
    use pretty_assertions::assert_eq;
    use tracing_tracy::client::PlotFormat;

    /// The format heuristic classifies the display unit from the path, checking
    /// percentage before memory so `mem_usage` (a percentage in Bevy's system
    /// diagnostics) is not mistaken for a raw byte count.
    #[test]
    fn plot_format_is_inferred_from_the_path() {
        assert_eq!(plot_format_for("fps"), PlotFormat::Number);
        assert_eq!(plot_format_for("frame_time"), PlotFormat::Number);
        assert_eq!(plot_format_for("frame_count"), PlotFormat::Number);
        assert_eq!(plot_format_for("system/cpu_usage"), PlotFormat::Percentage);
        // Memory reported as a fraction of total is a percentage, not bytes.
        assert_eq!(plot_format_for("system/mem_usage"), PlotFormat::Percentage);
        assert_eq!(plot_format_for("store/bytes"), PlotFormat::Memory);
        assert_eq!(plot_format_for("heap_memory"), PlotFormat::Memory);
    }
}
