//! The boundary between a running script and the world it runs in.

use crate::library::BuiltinId;
use crate::vm::linkset_data::{LinksetData, LinksetId};
use crate::vm::report::{Speech, Whereabouts};

/// Which script instance is calling, as the host knows it.
///
/// The host mints one per instance when it adds the instance to an
/// [`Engine`](crate::vm::Engine), so the runtime never learns what an object,
/// an inventory item or an agent is. The engine serves instances in the order
/// of their ids, so a host that wants a particular round-robin order — the
/// book's "entity index, then script item id" — mints ids in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CallerId(pub u64);

/// What a script can reach of the world: every library function that touches
/// anything outside the script goes through here.
///
/// The host is a thin translation onto the region; a method with logic in it
/// is a library function in the wrong crate. The trait grows with the
/// library: each tranche adds the methods its functions need (the book
/// chapter `simulator/lsl-engine.md` sketches the first ones). Events travel
/// the other way, through [`Engine::post`](crate::vm::Engine::post).
pub trait Host {
    /// LSL's legacy `print(text)`. Second Life writes it to the simulator's
    /// own log, where no resident sees it, so a host logs it or records it and
    /// never turns it into chat.
    fn print(&mut self, caller: CallerId, text: &str);

    /// A library function declared a stub answered a call with its type's
    /// default. The script carries on as though the call happened; the host
    /// says so, once or every time, so nobody mistakes the default for the
    /// function's real answer.
    fn stubbed(&mut self, caller: CallerId, id: BuiltinId);

    /// The script left a state: whatever the host holds for it that a state
    /// change releases goes now — its listens (`llListen`) and the controls
    /// it took (`llTakeControls`). Called after the old state's `state_exit`
    /// and before the new one's `state_entry`.
    fn left_state(&mut self, caller: CallerId);

    /// The script called `name` in the same prim as `caller` — the one
    /// `llGetScriptState(name)` asks about — or [`None`] when the prim's
    /// inventory has no script by that name.
    fn script_named(&self, caller: CallerId, name: &str) -> Option<CallerId>;

    /// The script's object says `speech`. The host hands it to the region's
    /// chat fan-out: the avatars within [`Speech::to_avatars`] get it as
    /// chat from the object (on `DEBUG_CHANNEL`, as the viewer's debug
    /// chat), and the `listen`s within [`Speech::to_scripts`] get it as an
    /// event. The runtime says a run-time error and a library call's refusal
    /// this way, already worded and flood-limited.
    fn say(&mut self, caller: CallerId, speech: &Speech);

    /// The script's object tells its owner `text` and nobody else, wherever
    /// in the region the owner is — `llOwnerSay`'s reach. The runtime tells an
    /// owner this way what YEngine reports to the owner alone (an exhausted
    /// heap).
    fn owner_say(&mut self, caller: CallerId, text: &str);

    /// Where the script `caller` is, for the words of its error lines and
    /// the object its flood is counted against.
    fn whereabouts(&self, caller: CallerId) -> Whereabouts;

    /// A random number for `caller`, every value of the 32 bits equally
    /// likely (`llListRandomize`'s shuffle). It comes from the region's
    /// seeded source, never from the machine, so two runs of a scenario from
    /// one seed draw the same numbers (`roadmap/context/lsl.md`, the
    /// determinism rule).
    fn random_bits(&mut self, caller: CallerId) -> u32;

    /// The linkset the script `caller` is in. Two scripts share a linkset
    /// data store, and both get the `linkset_data` event of a change to it,
    /// exactly when the host answers the same id for them.
    fn linkset(&self, caller: CallerId) -> LinksetId;

    /// The linkset data store of the linkset `caller` is in
    /// (`llLinksetDataWrite` and its family). It is the linkset's, kept by
    /// the host: it survives the script's reset and travels with the object.
    /// A host for an OpenSim region, whose engines compile none of the
    /// functions, may keep one empty store for every caller.
    fn linkset_data(&mut self, caller: CallerId) -> &mut LinksetData;
}
