//! A viewer network diagnostic streamed to Tracy: the number of live region
//! **circuits**.
//!
//! A circuit is one connection to a simulator — the current (root) region, each
//! discovered neighbour, and any teleport-destination child circuit. In the ECS
//! world model every such connection is an entity carrying an [`SlRegion`]
//! component (plus `SlCurrentRegion` for the root circuit or `SlNeighbor` for a
//! child circuit), spawned when the circuit is established and despawned when it
//! is torn down — so counting `With<SlRegion>` entities is the authoritative
//! live-circuit count.
//!
//! This is registered as an ordinary [`bevy::diagnostic::Diagnostic`], so
//! [`crate::tracy_plots`] streams it to the profiler as a plot alongside the
//! others with no extra wiring. Two things it makes readable:
//!
//! * **Normalisation.** Global counters (entity count, draw calls, …) rezzing a
//!   multi-region view are only comparable once divided by the number of regions
//!   in the view; this plot is that divisor, so a per-region figure is
//!   `<global plot> / net/circuits` read straight off the timeline.
//! * **Regime changes.** A circuit opening or closing (region crossing,
//!   teleport, a neighbour coming into / dropping out of view) discontinuously
//!   changes what the other plots mean. Besides the step in this plot, each
//!   change drops a Tracy **message** onto the timeline, so a jump in another
//!   plot that lines up with one is immediately identifiable as a circuit-count
//!   artefact rather than a real per-region regression.
//!
//! Compiled only under `profile-tracy` (it exists solely to feed the profiler,
//! and the timeline message uses the Tracy client directly).

use bevy::diagnostic::{Diagnostic, DiagnosticPath, Diagnostics, RegisterDiagnostic as _};
use bevy::prelude::*;
use sl_client_bevy::SlRegion;
use tracing_tracy::client::Client;

/// Diagnostic path for the live circuit count. [`crate::tracy_plots`] streams
/// any registered diagnostic, so this needs no bespoke plotting code.
pub(crate) const NET_CIRCUITS: DiagnosticPath = DiagnosticPath::const_new("net/circuits");

/// Measure the live circuit count each frame and annotate every change.
///
/// One measurement per frame from the region-entity count; on a change from the
/// previous frame, a Tracy message marks the connect/disconnect on the timeline.
/// The first observation only seeds `last` (there is no prior value to diff
/// against), and the message is skipped when no profiler is attached — the plot
/// step still records it, and the pre-connect baseline is irrelevant to a
/// capture taken later.
fn measure_circuits(
    mut diagnostics: Diagnostics,
    regions: Query<(), With<SlRegion>>,
    mut last: Local<Option<usize>>,
) {
    let count = regions.iter().count();
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "a viewer never holds enough circuits for the f64 mantissa to lose a count"
    )]
    diagnostics.add_measurement(&NET_CIRCUITS, || count as f64);

    if *last != Some(count) {
        if let (Some(prev), Some(client)) = (*last, Client::running()) {
            let verb = if count > prev { "opened" } else { "closed" };
            client.message(
                &format!("circuit {verb}: {prev} -> {count} region circuit(s)"),
                0,
            );
        }
        *last = Some(count);
    }
}

/// Registers the [`NET_CIRCUITS`] diagnostic and its per-frame measurement.
///
/// Added under `profile-tracy` (see [`crate::tracy_plots`]); the measurement
/// runs in [`Update`], before the `Last` streaming system samples it.
pub(crate) struct NetDiagnosticsPlugin;

impl Plugin for NetDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.register_diagnostic(Diagnostic::new(NET_CIRCUITS).with_suffix(" circuits"))
            .add_systems(Update, measure_circuits);
    }
}
