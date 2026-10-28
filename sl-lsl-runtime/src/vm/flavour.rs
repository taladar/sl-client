//! Which grid's script engine a region imitates where the grids measurably
//! differ.

/// The script engine a region imitates.
///
/// The value model and the library are Second Life's whatever this says; the
/// flavour decides only what the grids were measured to do differently with
/// a script that goes wrong (book chapter `gridspec/lsl.md`, § Run-time
/// errors): the words of a run-time error and who hears them, whether the
/// faulted script stops, and the flood limit on its error lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EngineFlavour {
    /// Second Life's Mono. A fault stops the script and says two lines that
    /// a script within say range hears; `llSetScriptState(TRUE)` alone does
    /// not bring it back, a reset does.
    #[default]
    SecondLife,
    /// OpenSim's YEngine. A fault says one line that reaches avatars but no
    /// script, and leaves the script reading as running while it answers
    /// nothing until it is reset.
    YEngine,
    /// OpenSim's XEngine. A fault says one line that reaches avatars but no
    /// script, ends the event it happened in, and the script runs on.
    XEngine,
}

impl EngineFlavour {
    /// Whether this is one of OpenSim's engines.
    #[must_use]
    pub const fn is_opensim(self) -> bool {
        matches!(self, Self::YEngine | Self::XEngine)
    }
}
