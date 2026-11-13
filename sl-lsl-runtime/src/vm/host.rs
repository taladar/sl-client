//! The boundary between a running script and the world it runs in.

use sl_types::lsl::Vector;

use crate::library::BuiltinId;
use crate::vm::Fault;
use crate::vm::comms::{
    Dialog, InstantMessage, InvalidLinkTarget, Links, LoadUrl, MapDestination, Overheard,
};
use crate::vm::contents::{Item, Offer, RemoteLoad, Rez, RezRefusal};
use crate::vm::economy::{MoneyRefusal, PayPrice, Payment};
use crate::vm::experiences::{
    Admission, ExperienceDetails, ExperienceError, ExperienceQuestion, KeyValueStore,
    ScriptExperience, SitRefusal,
};
use crate::vm::land::{Environment, LandChange, Parcel, ParcelMedia, Region, SimData};
use crate::vm::linkset_data::{LinksetData, LinksetId};
use crate::vm::ossl::{Simulator, Standing};
use crate::vm::permissions::Question;
use crate::vm::prim::{MoveTarget, ObjectDetails, Prim, SoundCue};
use crate::vm::report::{Speech, Whereabouts};
use crate::vm::seating::{Placement, Seat};
use crate::vm::sensing::{RayHit, Sensed};

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
    /// chat fan-out for the avatars within [`Speech::to_avatars`], who get it
    /// as chat from the object (on `DEBUG_CHANNEL`, as the viewer's debug
    /// chat), and answers which scripts are within [`Speech::to_scripts`] —
    /// all but the scripts of the speaking prim itself, which never hears its
    /// own line — for the engine to offer it to; [`None`] when none is. The
    /// runtime says a run-time error and a library call's refusal this way,
    /// already worded and flood-limited, and `llWhisper`, `llSay`, `llShout`
    /// and `llRegionSay` already cut.
    fn say(&mut self, caller: CallerId, speech: &Speech) -> Option<Overheard>;

    /// `llRegionSayTo`: the script's object says `text` on `channel` to
    /// `target` alone, wherever in the region it is. An avatar on channel 0
    /// gets it as chat from the object, and on `DEBUG_CHANNEL` as a script
    /// error — how the engine tells an owner alone what went wrong; on
    /// another channel the scripts of the objects it wears are offered it; a
    /// prim's scripts are offered it on any channel. The host delivers it to
    /// the avatar and answers the scripts to offer it to, for the engine to
    /// offer; [`None`] when there are none. The runtime has dropped a target
    /// that is no key, `NULL_KEY` and the caller's own prim, and cut the text.
    fn say_to(
        &mut self,
        caller: CallerId,
        target: &str,
        channel: i32,
        text: &str,
    ) -> Option<Overheard>;

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

    /// The linkset `caller` is in, prim by prim with the scripts in each, and
    /// the avatars sitting on it: what every function that addresses a prim
    /// by its link number reads.
    fn links(&self, caller: CallerId) -> Links;

    /// `llCreateLink`: link the object one of whose prims is keyed `target`
    /// with `caller`'s — `caller`'s object the root when `parent` is true,
    /// `target`'s when it is false — the script holding
    /// `PERMISSION_CHANGE_LINKS`. The host links them once control is back
    /// with the grid, the way a viewer's link of the two would, and raises
    /// `changed(CHANGED_LINK)` as the grid does.
    ///
    /// # Errors
    ///
    /// [`InvalidLinkTarget`] when `target` names no prim of the region, or
    /// one of an object worn or owned by somebody else than `caller`'s.
    fn create_link(
        &mut self,
        caller: CallerId,
        target: &str,
        parent: bool,
    ) -> Result<(), InvalidLinkTarget>;

    /// `llBreakLink`: take the prim keyed `prim`, which [`Self::links`]
    /// named, out of `caller`'s linkset, as a viewer's delink of it would,
    /// once control is back with the grid. Nothing for a worn object.
    fn break_link(&mut self, caller: CallerId, prim: &str);

    /// `llBreakAllLinks`: take every prim of `caller`'s linkset out of it,
    /// once control is back with the grid. Nothing for a worn object.
    fn break_all_links(&mut self, caller: CallerId);

    /// `llAllowInventoryDrop`: whether anybody, not only those who may
    /// modify it, may drop an item into `caller`'s prim — the prim's
    /// allow-drop flag, which a viewer reads off its object update.
    fn allow_inventory_drop(&mut self, caller: CallerId, allow: bool);

    /// The script's object tells its owner `text` and nobody else, wherever
    /// in the region the owner is — `llOwnerSay`'s reach; an owner in
    /// another region gets nothing. The runtime tells an owner this way what
    /// YEngine reports to the owner alone (an exhausted heap), and
    /// `llOwnerSay`'s text, already cut.
    fn owner_say(&mut self, caller: CallerId, text: &str);

    /// Where the script `caller` is, for the words of its error lines and
    /// the object its flood is counted against.
    fn whereabouts(&self, caller: CallerId) -> Whereabouts;

    /// The script `caller` hit the run-time error `fault`: it is already
    /// stopped or held as its engine does, and its error lines already said
    /// (or dropped by Second Life's flood limit). The whole fault, with the
    /// source position no grid tells a resident, for the host's own record —
    /// what a test harness reports a broken script by.
    fn faulted(&mut self, caller: CallerId, fault: &Fault);

    /// A random number for `caller`, every value of the 32 bits equally
    /// likely (`llListRandomize`'s shuffle). It comes from the region's
    /// seeded source, never from the machine, so two runs of a scenario from
    /// one seed draw the same numbers (`roadmap/context/lsl.md`, the
    /// determinism rule).
    fn random_bits(&mut self, caller: CallerId) -> u32;

    /// A fresh id from the region's counter of prim local ids, which OpenSim
    /// hands a position or rotation target as its handle (`AllocateIntId`):
    /// the region's next local id, never one handed out before.
    fn mint_scene_id(&mut self) -> i32;

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

    /// `llPassTouches`: whether a touch on the prim `caller` is in also
    /// reaches its linkset's root is now `pass`, as the script gave it
    /// (`PASS_IF_NOT_HANDLED` 0, `PASS_ALWAYS` 1, `PASS_NEVER` 2) — the
    /// prim's, so a reset leaves it. What a value means is the grid's: the
    /// host routes the touches.
    fn pass_touches(&mut self, caller: CallerId, pass: i32);

    /// `llPassCollisions`: whether a collision of the prim `caller` is in
    /// also reaches its linkset's root is now `pass`, as the script gave it
    /// (`PASS_IF_NOT_HANDLED` 0, `PASS_ALWAYS` 1, `PASS_NEVER` 2) — the
    /// prim's, so a reset leaves it. The host routes the collisions.
    fn pass_collisions(&mut self, caller: CallerId, pass: i32);

    /// `llMoveToTarget` and `llStopMoveToTarget`: the object `caller` is in
    /// is drawn towards `target` while it is physical, or no longer drawn for
    /// [`None`]. The object's, so a reset leaves it; the library has already
    /// refused what the grid refuses.
    fn move_to_target(&mut self, caller: CallerId, target: Option<MoveTarget>);

    /// `llSetBuoyancy`: the share of gravity the object `caller` is in no
    /// longer feels while physical — 1 floats, above 1 rises. The object's.
    fn set_buoyancy(&mut self, caller: CallerId, buoyancy: f32);

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

    /// `llAttachToAvatarTemp`: wear `caller`'s object on its owner's `point`
    /// as [`attach`](Self::attach) does, but filed as no item — its
    /// `AttachItemID` its own id — and gone when it comes off. A host that
    /// keeps no inventory wears it as any other.
    fn attach_temp(&mut self, caller: CallerId, point: i32) {
        self.attach(caller, point);
    }

    /// `llDetachFromAvatar`: take `caller`'s worn object off, back into its
    /// wearer's inventory.
    fn detach(&mut self, caller: CallerId);

    /// Where `caller`'s prim stands (`llGetPos`, `llGetLocalPos`,
    /// `llGetRootPosition`).
    fn placement(&self, caller: CallerId) -> Placement;

    /// `llSetPos`: moves `caller`'s prim to `local`, its position from its
    /// parent as [`Placement::local`] reads it — an unworn root's region
    /// position, a worn root's offset on its attachment point, a child's
    /// offset from its root. The library has capped the move already; the
    /// host keeps an unworn root on or above the ground under it, as both
    /// grids do, and a later [`Self::placement`] reads the new position.
    fn set_position(&mut self, caller: CallerId, local: Vector);

    /// The prim keyed `prim` as the library reads it, or [`None`] when the
    /// region has no prim of that key. `caller` names the script asking:
    /// its linkset decides nothing here, since a library function resolves
    /// a link number to a key ([`Self::links`]) before it asks, and OpenSim's
    /// functions read prims of other objects by key.
    fn prim(&self, caller: CallerId, prim: &str) -> Option<Prim>;

    /// The tick the prim keyed `prim` came into the region in — rezzed,
    /// whether new, from an inventory or back from a take, or there when
    /// the region started (tick 0) — or [`None`] when the host does not
    /// keep it. Second Life counts it as the prim's first bounding-box
    /// refresh (`prim/box-beat`).
    fn rezzed_in(&self, prim: &str) -> Option<crate::vm::Tick>;

    /// Writes `state` back as the prim keyed [`Prim::key`]: everything a
    /// setter changed, which the library has clamped as the grid clamps it.
    /// The host keeps what it was given, so a later [`Self::prim`] reads the
    /// values set; a move of an unworn root it keeps out of the ground and in
    /// the region, as [`Self::set_position`] does. The `changed` the write
    /// raises the engine queues itself, at the moment each flavour queues it
    /// (`events/order`, `events/latency`).
    fn set_prim(&mut self, caller: CallerId, state: &Prim);

    /// `llSetRegionPos`: moves `caller`'s object so its root stands at
    /// `position` in the region, in one move however far; the host keeps it
    /// out of the ground. Where the root ended, or [`None`] when it does not
    /// move: a position outside the region, or a worn object.
    fn set_region_position(&mut self, caller: CallerId, position: Vector) -> Option<Vector>;

    /// The keys of every prim of the object one of whose prims is keyed
    /// `key`, its root first and its children in link order; empty for a key
    /// no prim of the region has (`llGetBoundingBox` of another object).
    fn linkset_of(&self, key: &str) -> Vec<String>;

    /// What `llGetObjectDetails` reads of the object or avatar keyed `key`,
    /// or [`None`] when the region has neither.
    fn object_details(&self, caller: CallerId, key: &str) -> Option<ObjectDetails>;

    /// The asset key of the item named `name` of the asset type `asset_type`
    /// (`INVENTORY_TEXTURE`, `INVENTORY_SOUND`, …) in `caller`'s prim, or
    /// [`None`] when its contents have none: how a setter given a name
    /// rather than a key finds the texture or the sound.
    fn inventory_key(&self, caller: CallerId, name: &str, asset_type: i32) -> Option<String>;

    /// Plays or preloads a sound from the prim keyed `prim`, which is
    /// `caller`'s or one of its linkset: the viewers in earshot are sent it,
    /// and the region keeps nothing of it. What a prim plays on is in
    /// [`Prim::sound`], written with [`Self::set_prim`].
    fn sound(&mut self, caller: CallerId, prim: &str, cue: &SoundCue);

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

    /// Every item of `caller`'s prim, in any order: the library orders a
    /// listing as its flavour does.
    fn contents(&self, caller: CallerId) -> Vec<Item>;

    /// `llRemoveInventory`: take the item named `name` out of `caller`'s
    /// prim, advancing its contents serial so a viewer's Contents tab
    /// refreshes, and raise `changed(bits)` in the prim's scripts once per
    /// element of `changed` — nothing on Second Life, `CHANGED_INVENTORY` on
    /// OpenSim. `false` when the prim holds no such item. A script removed
    /// loses its instance, the caller's own included.
    fn remove_item(&mut self, caller: CallerId, name: &str, changed: &[i32]) -> bool;

    /// `llGiveInventory` and `llGiveInventoryList` to a prim: copy the items
    /// named `items`, all of them in `caller`'s prim, into the prim keyed
    /// `target`, each under its name or the first `name N` the target does
    /// not hold, a script arriving stopped, and raise
    /// `changed(CHANGED_INVENTORY)` in the target's scripts — on both grids.
    /// `false` when the region has no prim keyed `target`, or `caller`'s
    /// owner may not drop into it.
    fn give_to_prim(&mut self, caller: CallerId, target: &str, items: &[String]) -> bool;

    /// `llGiveInventory` and `llGiveInventoryList` to an avatar: put `offer`
    /// to it, whether or not it is in the region, and file the items in its
    /// inventory when it accepts. `false` when the key is nobody's.
    fn offer_items(&mut self, caller: CallerId, offer: &Offer) -> bool;

    /// `llSetRemoteScriptAccessPin`: the pin a script of another prim of the
    /// same owner must give to load a script into `caller`'s prim; 0 for
    /// none. The prim's, so a reset leaves it.
    fn set_remote_pin(&mut self, caller: CallerId, pin: i32);

    /// The remote script access pin of the prim keyed `prim` and its owner,
    /// or [`None`] when the region has no such prim.
    fn remote_pin(&self, prim: &str) -> Option<(i32, String)>;

    /// `llRemoteLoadScriptPin`: copy the script `load` names out of
    /// `caller`'s prim into the prim [`RemoteLoad::target`] names, replacing
    /// a script of that name, and start it with the start parameter, running
    /// or not — afresh, with no `on_rez`. The library has checked the pin.
    fn load_script(&mut self, caller: CallerId, load: &RemoteLoad);

    /// `llRezObject`, `llRezAtRoot`, `llRezObjectWithParams`: put the object
    /// `rez` names out of `caller`'s prim into the region, its scripts
    /// brought back as they were taken and told of the rez with `on_rez`, and
    /// answer the key of its root. Once the object's scripts have run their
    /// `on_rez`, the host tells the rezzer with
    /// [`Engine::object_rezzed`](crate::vm::Engine::object_rezzed) — which is
    /// why the rezzer's `object_rez` arrives first on both grids though the
    /// child's `on_rez` ran before it.
    ///
    /// # Errors
    ///
    /// [`RezRefusal`] when nothing is rezzed.
    fn rez(&mut self, caller: CallerId, rez: &Rez) -> Result<String, RezRefusal>;

    /// `llDerezObject(id, DEREZ_DIE)`: take the object keyed `object` out of
    /// the region, as `llDie` would, once control is back with the grid —
    /// only an object `caller`'s object rezzed. Whether it goes.
    fn derez(&mut self, caller: CallerId, object: &str) -> bool;

    /// The text of the notecard whose asset is keyed `asset`, read now, after
    /// which the region holds it read — what Second Life's
    /// `llGetNotecardLineSync` reads; [`None`] when there is no such
    /// notecard.
    fn notecard_text(&mut self, asset: &str) -> Option<String>;

    /// The text of the notecard keyed `asset` if the region has read it
    /// already ([`Self::notecard_text`]), else [`None`].
    fn notecard_read(&self, asset: &str) -> Option<String>;

    /// The parcel of `caller`'s region at `(x, y)`, or [`None`] where the
    /// region has none — every point outside it: how a position off the
    /// region is read is the library's.
    fn parcel_at(&self, caller: CallerId, x: f32, y: f32) -> Option<Parcel>;

    /// The region `caller` is in.
    fn region(&self, caller: CallerId) -> Region;

    /// What `llRequestSimulatorData` reads of the grid's region called
    /// `name`, matched without regard to case; [`None`] for a name no region
    /// of the grid has.
    fn sim_data(&self, caller: CallerId, name: &str) -> Option<SimData>;

    /// The ground's height at `(x, y)` of `caller`'s region, which lies
    /// within it, between the heightfield's points as the grid interpolates.
    fn ground(&self, caller: CallerId, x: f32, y: f32) -> f32;

    /// The wind at `position` of `caller`'s region.
    fn wind(&self, caller: CallerId, position: &Vector) -> Vector;

    /// The environment over `position` of `caller`'s region — the parcel's,
    /// or for [`None`] the region's — at UNIX time `unix`: its day and how the
    /// sun and the moon are turned then.
    fn environment(&self, caller: CallerId, position: Option<&Vector>, unix: i64) -> Environment;

    /// Make `change` to the parcel at `position` or to the estate, which the
    /// library has checked `caller`'s owner may make; whether it was made.
    fn change_land(&mut self, caller: CallerId, position: &Vector, change: &LandChange) -> bool;

    /// The media of the parcel at `position` of `caller`'s region.
    fn parcel_media(&self, caller: CallerId, position: &Vector) -> ParcelMedia;

    /// The region's statistics in `SIM_STAT_*` order (0–26), as its
    /// `SimStats` reports them.
    fn sim_stats(&self, caller: CallerId) -> Vec<f32>;

    /// The values `llGetEnvironment`'s `SKY_*` or `WATER_*` rule `rule` reads
    /// of the environment over `position`, in the rule's order — the shape of
    /// the grid's settings record is the host's — or [`None`] for a rule that
    /// is none.
    fn environment_rule(
        &self,
        caller: CallerId,
        position: &Vector,
        rule: i32,
    ) -> Option<Vec<crate::value::Element>>;

    /// `llHTTPRequest`: send `request`, checked and with every header it
    /// goes out with, from `caller`'s prim. The host hands the answer back
    /// later with [`crate::vm::Engine::http_response`] — a host that sends
    /// nothing off the grid answers with a stated failure, never silence.
    fn send_http(&mut self, caller: CallerId, request: &crate::vm::http::Outbound);

    /// `llRequestURL`, `llRequestSecureURL`: a URL for `caller`, keyed `key`
    /// ([`crate::vm::http::granted_url`] spells it), or [`None`] when none
    /// is granted (no URL left, no HTTPS listener).
    fn request_url(&mut self, caller: CallerId, key: &str, secure: bool) -> Option<String>;

    /// `llReleaseURL`: `url`, if it is `caller`'s, is free again.
    fn release_url(&mut self, caller: CallerId, url: &str);

    /// `llGetFreeURLs`: how many more URLs `caller`'s land allows.
    fn free_urls(&self, caller: CallerId) -> i32;

    /// `llHTTPResponse`: send `answered` back for the request `request` that
    /// arrived at one of `caller`'s URLs.
    fn answer_http(&mut self, caller: CallerId, request: &str, answered: crate::vm::http::Answered);

    /// The `llHTTPRequest` throttle of `caller`'s prim, kept by the host for
    /// the prim.
    fn http_throttle(&mut self, caller: CallerId) -> &mut crate::vm::http::Throttle;

    /// The `host:port` the grid's script URLs are reached at: the one address
    /// on the simulator's own network a script's request may go to.
    fn url_authority(&self, caller: CallerId) -> String;

    /// The avatar `agent` if it is in the region (`llGetAgentSize`,
    /// `llGetAgentInfo`, `llGetAnimation`, the names).
    fn agent(&self, caller: CallerId, agent: &str) -> Option<crate::vm::avatars::Agent>;

    /// Any avatar of the grid, by key (`llRequestAgentData`,
    /// `llRequestDisplayName`, `llRequestUsername`), or [`None`] for a key no
    /// account has.
    fn agent_record(
        &self,
        caller: CallerId,
        agent: &str,
    ) -> Option<crate::vm::avatars::AgentRecord>;

    /// The key of the avatar of the grid named `name` — a legacy name or a
    /// user name — and whether it is in the region (`llName2Key`,
    /// `llRequestUserKey`); [`None`] when no account has that name.
    fn agent_named(&self, caller: CallerId, name: &str) -> Option<(String, bool)>;

    /// `llStartAnimation` (`start`) or `llStopAnimation` of the animation
    /// asset `animation` on `agent`, which granted the script.
    fn animate(&mut self, caller: CallerId, agent: &str, animation: &str, start: bool);

    /// `llSetAnimationOverride`: `agent`'s movement state `state` played as
    /// `animation` (its name as the script gave it, and its asset), or with
    /// [`None`] as it plays by default (`llResetAnimationOverride`).
    fn set_animation_override(
        &mut self,
        caller: CallerId,
        agent: &str,
        state: &str,
        animation: Option<(&str, &str)>,
    );

    /// `llGetAnimationOverride`: the name of what overrides `agent`'s
    /// movement state `state`, or [`None`] when nothing does.
    fn animation_override(&self, caller: CallerId, agent: &str, state: &str) -> Option<String>;

    /// `llStartObjectAnimation` (`start`) or `llStopObjectAnimation` of the
    /// animation named `name` (asset `animation`) on `caller`'s object.
    fn object_animate(&mut self, caller: CallerId, name: &str, animation: &str, start: bool);

    /// `llGetObjectAnimationNames`: what `caller`'s object plays.
    fn object_animations(&self, caller: CallerId) -> Vec<String>;

    /// `llTakeControls`, `llReleaseControls`: send `agent`'s viewer
    /// `changes`, and route its keys to the script — `taken` controls of it,
    /// or none with [`None`] — through [`crate::vm::Engine::controls`].
    fn control_changes(
        &mut self,
        caller: CallerId,
        agent: &str,
        changes: &[crate::vm::avatars::ControlChange],
        taken: Option<i32>,
    );

    /// `llSetCameraParams`: send `agent`'s viewer the follow-camera
    /// properties `rules` of `caller`'s object.
    fn set_camera(
        &mut self,
        caller: CallerId,
        agent: &str,
        rules: &[crate::vm::avatars::CameraRule],
    );

    /// `llClearCameraParams`: clear them.
    fn clear_camera(&mut self, caller: CallerId, agent: &str);

    /// Where `agent`'s camera is and how it is turned (`llGetCameraPos`,
    /// `llGetCameraRot`), or [`None`] when it is not in the region.
    fn camera(&self, caller: CallerId, agent: &str) -> Option<(Vector, sl_types::lsl::Rotation)>;

    /// `llTeleportAgent` and its family: send `agent` to `destination`.
    fn teleport_agent(
        &mut self,
        caller: CallerId,
        agent: &str,
        destination: &crate::vm::avatars::Destination,
    );

    /// `llGetAttachedList`: the keys of the objects `agent` wears that a
    /// script may see, or [`None`] when it is not in the region.
    fn attached_list(&self, caller: CallerId, agent: &str) -> Option<Vec<String>>;

    /// The key of the map texture of the grid's region named, or keyed,
    /// `region` — of the caller's own region for an empty `region` — or
    /// [`None`] for a region the grid does not know (`osGetMapTexture`,
    /// `osGetRegionMapTexture`).
    fn map_texture(&self, caller: CallerId, region: &str) -> Option<String>;

    /// `osMakeNotecard`: write a notecard of `text` into `caller`'s prim
    /// under `name`, or the first `name N` the prim does not hold, described
    /// `description`, its owner the prim's owner and its creator too, and
    /// raise `changed(bits)` in the prim's scripts once per element of
    /// `changed`.
    fn make_notecard(
        &mut self,
        caller: CallerId,
        name: &str,
        description: &str,
        text: &str,
        changed: &[i32],
    );

    /// The region's calendar time at its tick 0, in microseconds since the
    /// UNIX epoch: what every date a script reads counts on from by its
    /// ticks, so it follows from the seed and the ticks and never from the
    /// machine (`book/src/simulator/determinism.md`).
    fn epoch_micros(&self) -> i64;

    /// The offset from UTC, in seconds, of the time zone the simulator's host
    /// keeps at UNIX time `unix` — what OpenSim's `DateTime.Now` reads
    /// (`llGetWallclock`, `llGetTimeOfDay`).
    fn local_offset(&self, unix: i64) -> i32;

    /// The offset in seconds of the day cycle of the environment over
    /// `caller`'s prim (OpenSim's `DayOffset`, which its apparent time adds).
    fn day_offset(&self, caller: CallerId) -> f32;

    /// Every object and avatar of the region `caller` is in, as a sweep
    /// weighs it — an object by its root — in any order: which a sweep
    /// returns, and in what order, is the library's.
    fn sense(&self, caller: CallerId) -> Vec<Sensed>;

    /// Every surface the segment from `start` to `end` crosses, cut where it
    /// leaves the region, nearest `start` first: prims (where it enters them,
    /// or leaves one it starts inside), avatars and the ground. Which a
    /// script is told of is the library's.
    fn cast_ray(&self, caller: CallerId, start: &Vector, end: &Vector) -> Vec<RayHit>;
}
