//! Which grid's script engine a region imitates where the grids measurably
//! differ.

/// The script engine a region imitates.
///
/// The value model is Second Life's whatever this says — OpenSim's doubles
/// are not modelled, so a float is an `f32` and `(string)` writes Second
/// Life's digits on every grid. The flavour decides what the grids were
/// measured to do differently (book chapter `gridspec/lsl.md`): with a
/// script that goes wrong (§ Run-time errors) — the words of a run-time
/// error and who hears them, whether the faulted script stops, the flood
/// limit on its error lines; how a script's memory is counted (§ Memory and
/// limits); and the library functions whose answers differ (§ Strings,
/// lists, maths and rotations), OpenSim's ported from its `LSL_Api.cs`.
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
