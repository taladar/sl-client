//! The boundary between a running script and the world it runs in.

use crate::library::BuiltinId;
use crate::vm::comms::{Dialog, InstantMessage, Links, LoadUrl, MapDestination};
use crate::vm::economy::{MoneyRefusal, PayPrice, Payment};
use crate::vm::experiences::{
    Admission, ExperienceDetails, ExperienceError, ExperienceQuestion, KeyValueStore,
    ScriptExperience, SitRefusal,
};
use crate::vm::linkset_data::{LinksetData, LinksetId};
use crate::vm::ossl::{Simulator, Standing};
use crate::vm::permissions::Question;
use crate::vm::report::{Speech, Whereabouts};
use crate::vm::seating::{Placement, Seat};

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
    /// change releases goes now — the controls it took (`llTakeControls`).
    /// Its listens are the instance's own and are already gone. Called after
    /// the old state's `state_exit` and before the new one's `state_entry`.
    fn left_state(&mut self, caller: CallerId);

    /// `llDie`: the object `caller` is in is to leave the region, every prim
    /// of it and every script in them. The host removes it once control is
    /// back with the grid, never from inside the call, and answers whether it
    /// will: `false` for an attachment, which `llDie` leaves where it is on
    /// every grid. When it answers `true` the calling script runs nothing
    /// more; when `false` it carries on with the next instruction.
    fn die(&mut self, caller: CallerId) -> bool;

    /// The script called `name` in the same prim as `caller` — the one
    /// `llGetScriptState(name)` asks about — or [`None`] when the prim's
    /// inventory has no script by that name.
    fn script_named(&self, caller: CallerId, name: &str) -> Option<CallerId>;

    /// The script's object says `speech`. The host hands it to the region's
    /// chat fan-out: the avatars within [`Speech::to_avatars`] get it as
    /// chat from the object (on `DEBUG_CHANNEL`, as the viewer's debug
    /// chat), and the scripts within [`Speech::to_scripts`] are offered it
    /// ([`Engine::hear`](crate::vm::Engine::hear)) — all but the scripts of
    /// the speaking prim itself, which never hears its own line. The runtime
    /// says a run-time error and a library call's refusal this way, already
    /// worded and flood-limited, and `llWhisper`, `llSay`, `llShout` and
    /// `llRegionSay` already cut.
    fn say(&mut self, caller: CallerId, speech: &Speech);

    /// `llRegionSayTo`: the script's object says `text` on `channel` to
    /// `target` alone, wherever in the region it is. An avatar on channel 0
    /// gets it as chat from the object; on another channel the scripts of the
    /// objects it wears are offered it; a prim's scripts are offered it on
    /// any channel. The runtime has dropped a target that is no key,
    /// `NULL_KEY` and the caller's own prim, and cut the text.
    fn say_to(&mut self, caller: CallerId, target: &str, channel: i32, text: &str);

    /// `llInstantMessage`: send `message` to its avatar, wherever it is —
    /// another region included, which `llOwnerSay` does not reach. A key that
    /// is nobody's is nothing, without complaint.
    fn instant_message(&mut self, caller: CallerId, message: &InstantMessage);

    /// `llDialog` and `llTextBox`: put `dialog` to its avatar, when the
    /// avatar is in the region. The reply is a line the avatar says on the
    /// dialog's channel, which the host offers the scripts in earshot as it
    /// does any line, under the avatar's key and name.
    fn dialog(&mut self, caller: CallerId, dialog: &Dialog);

    /// `llLoadURL`: offer the page to its avatar, when it is in the region.
    fn load_url(&mut self, caller: CallerId, offer: &LoadUrl);

    /// `llMapDestination`: open its avatar's world map at the place.
    fn map_destination(&mut self, caller: CallerId, destination: &MapDestination);

    /// The avatar wearing the object `caller` is in, or [`None`] when it is
    /// not an attachment (`llMapDestination` outside a touch).
    fn worn_by(&self, caller: CallerId) -> Option<String>;

    /// The linkset `caller` is in, prim by prim with the scripts in each, for
    /// `llMessageLinked`.
    fn links(&self, caller: CallerId) -> Links;

    /// The script's object tells its owner `text` and nobody else, wherever
    /// in the region the owner is — `llOwnerSay`'s reach; an owner in
    /// another region gets nothing. The runtime tells an owner this way what
    /// YEngine reports to the owner alone (an exhausted heap), and
    /// `llOwnerSay`'s text, already cut.
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

    /// The key of the avatar who owns the object `caller` is in. Second Life
    /// asks only the owner for `PERMISSION_DEBIT`.
    fn owner(&self, caller: CallerId) -> String;

    /// Whether `agent` sits on the object `caller` is in: what it asks of a
    /// seated avatar's controls, camera and animations is granted unasked.
    fn seated_on(&self, caller: CallerId, agent: &str) -> bool;

    /// `llRequestPermissions`: put `question` to its avatar's viewer as a
    /// `ScriptQuestion` naming the object and its owner. `false` when the
    /// avatar is not in the region, which puts nothing. The avatar's answer
    /// comes back through [`Engine::answer`](crate::vm::Engine::answer).
    fn ask_permissions(&mut self, caller: CallerId, question: &Question) -> bool;

    /// `llGiveMoney`: pay `payment` from the object's owner, the script
    /// holding `PERMISSION_DEBIT`. Nothing comes back to the script, whatever
    /// becomes of it; the balance replies are the host's.
    fn give_money(&mut self, caller: CallerId, payment: &Payment);

    /// `llTransferLindenDollars`: pay `payment` from the object's owner, the
    /// script holding `PERMISSION_DEBIT`, and say whether the money moved —
    /// the script's `transaction_result`.
    ///
    /// # Errors
    ///
    /// [`MoneyRefusal`] when the economy did not move it.
    fn transfer_money(&mut self, caller: CallerId, payment: &Payment) -> Result<(), MoneyRefusal>;

    /// `llSetPayPrice`: the pay layout of the prim `caller` is in is now
    /// `layout` — the prim's, so a reset leaves it. A viewer's
    /// `RequestPayPrice` is answered [`PayPrice::reply`].
    fn set_pay_price(&mut self, caller: CallerId, layout: PayPrice);

    /// The experience the script `caller` was compiled into (the upload's
    /// experience, kept on its inventory item), and whether it may run where
    /// the script is; [`None`] for a script compiled into none — every
    /// script, until an upload names one. A host for an OpenSim region, whose
    /// engines compile none of the experience functions, answers [`None`].
    fn experience(&self, caller: CallerId) -> Option<ScriptExperience>;

    /// What `llGetExperienceDetails` reads of the experience `id`, or
    /// [`None`] while the region has yet to look it up: Second Life answers
    /// an id it has not seen with an empty list, and a second later with the
    /// record — [`ExperienceDetails::invalid`] when no experience has the id
    /// (`experience/no-experience`). The host keeps what it has looked up.
    fn experience_details(&mut self, caller: CallerId, id: &str) -> Option<ExperienceDetails>;

    /// What `agent` has answered the experience `experience`: allowed,
    /// blocked or neither — the grid's record, the same for every script of
    /// the experience, wherever it is.
    fn admission(&self, agent: &str, experience: &str) -> Admission;

    /// `llRequestExperiencePermissions` of an avatar that has not answered
    /// the experience: put `question` to its viewer as a `ScriptQuestion`
    /// naming the experience. `false` when the avatar is not in the region,
    /// which puts nothing. The host records the answer for the experience —
    /// allowed or blocked, for every later request — and reports it through
    /// [`Engine::answer_experience`](crate::vm::Engine::answer_experience),
    /// as it reports [`ExperienceError::RequestTimedOut`] for a question
    /// nobody answered in five minutes.
    fn ask_experience(&mut self, caller: CallerId, question: &ExperienceQuestion) -> bool;

    /// The key-value store of the experience `experience`, kept by the grid
    /// for the experience: every object in it shares one, and it outlives
    /// them. [`ExperienceError::StoreDisabled`] when the region's store is
    /// off.
    ///
    /// # Errors
    ///
    /// The error a key-value call answers when the store cannot be reached.
    fn key_value_store(&mut self, experience: &str) -> Result<&mut KeyValueStore, ExperienceError>;

    /// `llSitOnLink`: sit `agent`, who has allowed the script's experience,
    /// on the sit target of the prim `link` of the object `caller` is in, or
    /// when that is taken on the next free one down the linkset.
    ///
    /// # Errors
    ///
    /// [`SitRefusal`] when nobody sits.
    fn sit_on_link(&mut self, caller: CallerId, agent: &str, link: i32) -> Result<(), SitRefusal>;

    /// The seat settings of the prim numbered `link` in the linkset `caller`
    /// is in (a number [`Self::links`] gave it, never a `LINK_*` constant),
    /// to read and change: its sit target, its sitter's camera and
    /// mouselook. [`None`] when the linkset has no prim of that number. The
    /// host keeps one per prim, empty until a script sets it; a reset of the
    /// script leaves it, and an avatar who sits there is placed by it.
    fn seat(&mut self, caller: CallerId, link: i32) -> Option<&mut Seat>;

    /// The avatar sitting on the sit target of the prim numbered `link` in
    /// `caller`'s linkset (`llAvatarOnSitTarget`), or [`None`]: for a prim
    /// with no target, one whose target nobody took — an avatar seated on
    /// the prim beside a sitter already on its target is not on it — and no
    /// prim of that number.
    fn avatar_on_sit_target(&self, caller: CallerId, link: i32) -> Option<String>;

    /// `llUnSit`: stand `agent` up when it sits on `caller`'s object, or —
    /// sitting anywhere — stands over land the object's owner owns or holds
    /// estate powers over. Anything else is nothing.
    fn unsit(&mut self, caller: CallerId, agent: &str);

    /// `llSetSitText` and `llSetTouchText`: what the viewer's pie menu says
    /// for sitting on (`sit` true) or touching `caller`'s prim, already cut.
    fn set_pie_text(&mut self, caller: CallerId, sit: bool, text: &str);

    /// The attachment point `caller`'s object is worn on (`llGetAttached`),
    /// `0` when it is not worn.
    fn attached(&self, caller: CallerId) -> i32;

    /// `llAttachToAvatar`: wear `caller`'s object on its owner's `point`, the
    /// owner having granted `PERMISSION_ATTACH` and being in the region; the
    /// region raises `attach` once it is worn. `0` is the point the object
    /// was last worn on.
    fn attach(&mut self, caller: CallerId, point: i32);

    /// `llDetachFromAvatar`: take `caller`'s worn object off, back into its
    /// wearer's inventory.
    fn detach(&mut self, caller: CallerId);

    /// Where `caller`'s prim stands (`llGetPos`, `llGetLocalPos`,
    /// `llGetRootPosition`).
    fn placement(&self, caller: CallerId) -> Placement;

    /// How many prims the object one of whose prims is keyed `object` has,
    /// the avatars sitting on it left out (`llGetObjectPrimCount`); [`None`]
    /// for a key no prim of the region has — an avatar's among them.
    fn prim_count(&self, object: &str) -> Option<i32>;

    /// What the owner of `caller`'s script is in the region — estate owner
    /// or manager, parcel owner, a god — as an OSSL `Allow_` line asks
    /// before an OpenSim function runs.
    fn standing(&self, caller: CallerId) -> Standing;

    /// The simulator and grid the script runs on, as OpenSim's identity
    /// readers answer them (`osGetSimulatorVersion`, `osGetGridName`).
    fn simulator(&self) -> Simulator;

    /// The size of the region `caller` is in, in metres (`osGetRegionSize`,
    /// whose third component is 0).
    fn region_size(&self, caller: CallerId) -> (u32, u32);

    /// The ground's height at the whole-metre point `(x, y)` of the region
    /// `caller` is in, which lies within [`Self::region_size`]
    /// (`osGetTerrainHeight`).
    fn terrain_height(&self, caller: CallerId, x: u32, y: u32) -> f32;

    /// The length in seconds of a day of the environment over `caller`'s
    /// prim (`osGetSunParam("day_length")`).
    fn day_length(&self, caller: CallerId) -> f32;

    /// The scripts in the prim keyed `prim` (`osMessageObject`'s target), or
    /// [`None`] when the region has no such prim.
    fn prim_scripts(&self, prim: &str) -> Option<Vec<CallerId>>;

    /// The address `agent` connects from, as `osGetAgentIP` gives it to a
    /// god: the IP alone, no port; empty when `agent` is not in the region.
    fn agent_ip(&self, agent: &str) -> String;
}
