//! The object-update stream as it came off the wire: which message carried
//! which objects, and what a viewer tells a simulator about its object cache.

use sl_wire::RegionHandle;

use sl_wire::RegionLocalObjectId;

/// The message an object arrived in, or left by.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ObjectUpdateForm {
    /// `ObjectUpdate`: every field of the object, spelled out.
    Full,
    /// `ObjectUpdateCompressed`: the same object packed into one blob.
    Compressed,
    /// `ObjectUpdateCached`: an id and a checksum, for a viewer to answer
    /// from its cache or ask for in full.
    Cached,
    /// `ImprovedTerseObjectUpdate`: motion alone.
    Terse,
    /// `KillObject`: the object is gone from the viewer's view.
    Kill,
}

/// One message of the object-update stream: its form and the objects it
/// named, in the order it named them.
///
/// The per-object events ([`Event::ObjectAdded`](crate::Event::ObjectAdded),
/// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated),
/// [`Event::ObjectRemoved`](crate::Event::ObjectRemoved)) say what became of
/// each object; this says how the simulator chose to send it — which is what
/// tells two simulators' streams apart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectStreamBatch {
    /// The region the message is about, or `0` where the message states none
    /// and the circuit's is not yet known.
    pub region_handle: RegionHandle,
    /// Whether the message came down a child-agent circuit (a neighbouring
    /// region) rather than the root circuit.
    pub child: bool,
    /// The message's form.
    pub form: ObjectUpdateForm,
    /// The objects it named.
    pub entries: Vec<ObjectStreamEntry>,
}

/// One object named by a message of the object-update stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObjectStreamEntry {
    /// The object's region-local id.
    pub local_id: RegionLocalObjectId,
    /// Whether the session already held the object when the message came: a
    /// full or compressed update of one is a change, not an arrival; a terse
    /// update or a kill of one it does not hold is about something it was
    /// never sent.
    pub known: bool,
    /// The checksum an `ObjectUpdateCached` states for the object.
    pub crc: Option<u32>,
    /// Whether the session answered an `ObjectUpdateCached` entry from what
    /// it held (`Some(true)`) or asked for the object in full
    /// (`Some(false)`).
    pub cache_hit: Option<bool>,
}

/// What a viewer tells a simulator about its object cache in the `Flags` of
/// its `RegionHandshakeReply`.
///
/// The reference viewer always sets
/// [`SUPPORTS_SELF_APPEARANCE`](Self::SUPPORTS_SELF_APPEARANCE), sets
/// [`CACHE_ALL`](Self::CACHE_ALL) while its object-cache culling is on (the
/// default), and sets [`CACHE_EMPTY`](Self::CACHE_EMPTY) when it has no cache
/// file for the region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionHandshakeReplyFlags(pub u32);

impl RegionHandshakeReplyFlags {
    /// "Send all cacheable objects": the viewer culls for itself, out of its
    /// cache, and wants the whole region rather than what the simulator's
    /// interest list would pick.
    pub const CACHE_ALL: u32 = 1;
    /// "The cache file is empty": no need to probe it with
    /// `ObjectUpdateCached`.
    pub const CACHE_EMPTY: u32 = 1 << 1;
    /// The viewer understands an `AvatarAppearance` about its own avatar
    /// (the reference viewer's `REGION_HANDSHAKE_SUPPORTS_SELF_APPEARANCE`).
    pub const SUPPORTS_SELF_APPEARANCE: u32 = 1 << 2;

    /// What a session sends unless told otherwise
    /// ([`Session::set_region_handshake_reply_flags`](crate::Session::set_region_handshake_reply_flags)):
    /// its cache is empty, because it keeps none between sessions, and it
    /// understands an appearance message about its own avatar — without
    /// which Second Life sends an arriving agent none. Not
    /// [`CACHE_ALL`](Self::CACHE_ALL): that is for a viewer that culls a whole
    /// region's objects for itself.
    pub const CLIENT_DEFAULT: Self = Self(Self::CACHE_EMPTY | Self::SUPPORTS_SELF_APPEARANCE);

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}
