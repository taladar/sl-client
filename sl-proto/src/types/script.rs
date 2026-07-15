//! Scripts and notifications: dialogs, permissions, alerts, mutes.

use sl_types::chat::ChatChannel;
use sl_types::key::{ExperienceKey, InventoryKey, ObjectKey, OwnerKey, TextureKey};
use sl_types::map::{RegionCoordinates, RegionName};
use sl_wire::ControlFlags;
use sl_wire::Direction;
use uuid::Uuid;

/// A scripted-object dialog (`llDialog`/`llTextBox`), parsed from a
/// `ScriptDialog`. Reply with
/// [`Session::reply_script_dialog`](crate::Session::reply_script_dialog), passing
/// the chosen button's index/label on [`chat_channel`](ScriptDialog::chat_channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDialog {
    /// The object id that raised the dialog (the reply target).
    pub object_id: ObjectKey,
    /// The object's name.
    pub object_name: String,
    /// The object owner's first name.
    pub owner_first_name: String,
    /// The object owner's last name.
    pub owner_last_name: String,
    /// The object owner's agent id (`None` if the sim did not include it).
    pub owner_id: Option<Uuid>,
    /// The dialog message text.
    pub message: String,
    /// The hidden chat channel the button reply is sent on.
    pub chat_channel: ChatChannel,
    /// The dialog's icon (texture id).
    pub image_id: TextureKey,
    /// The button labels, in order (the reply carries the chosen index/label).
    pub buttons: Vec<String>,
}

impl ScriptDialog {
    /// The magic single-button label an `llTextBox` uses instead of real
    /// buttons. When [`buttons`](Self::buttons) is exactly this, the object is
    /// requesting free-text input rather than a button choice.
    pub const TEXT_BOX_BUTTON: &'static str = "!!llTextBox!!";

    /// Whether this dialog is an `llTextBox` free-text prompt (a single
    /// [`TEXT_BOX_BUTTON`](Self::TEXT_BOX_BUTTON) button).
    #[must_use]
    pub fn is_text_box(&self) -> bool {
        self.buttons.len() == 1
            && self
                .buttons
                .first()
                .is_some_and(|button| button == Self::TEXT_BOX_BUTTON)
    }
}

// `ScriptPermissions` (the LSL `PERMISSION_*` request/grant bitfield) now lives
// in `sl_types::lsl`; re-exported here so the existing `sl_proto::…` path is
// unchanged.
pub use sl_types::lsl::ScriptPermissions;

/// The client's responsibility for a single granted [`ScriptPermissions`] flag.
///
/// The simulator stays authoritative for *every* permission — it enforces the
/// grant end-to-end and the client's record is only a mirror, never a security
/// boundary. This classifier exists for a driver's benefit (deciding what to
/// surface and whether to cooperate), not to branch `Session` behaviour: the
/// session takes **no** autonomous action on any granted flag.
///
/// There are only two roles — there is no autonomous-action role. `TELEPORT` is
/// [`RecordOnly`](Self::RecordOnly), not an action: a granted `llTeleportAgent`
/// teleports the agent server-side and arrives as an ordinary teleport handled
/// by the teleport state machine, so the client merely mirrors the grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRole {
    /// The simulator enforces the permission end-to-end; the client only
    /// mirrors the grant and takes no action (any effect arrives later on the
    /// ordinary message path). Covers `DEBIT`, `TRIGGER_ANIMATION`, `ATTACH`,
    /// `CHANGE_LINKS`, `TELEPORT`, `EXPERIENCE`, `SILENT_ESTATE_MANAGEMENT`,
    /// `OVERRIDE_ANIMATIONS`, and `RETURN_OBJECTS`.
    RecordOnly,
    /// The grant is inert until the runtime cooperates: routing the avatar's
    /// control inputs (`TAKE_CONTROLS`, surfaced via
    /// [`Event::ScriptControlChange`](crate::Event::ScriptControlChange)) or
    /// applying camera parameters (`TRACK_CAMERA` / `CONTROL_CAMERA`, surfaced
    /// via the follow-cam events). `sl-proto` surfaces the grant and tracks the
    /// live state, but initiates nothing.
    Cooperation,
}

impl PermissionRole {
    /// Classifies a single [`ScriptPermissions`] flag bit (one of the
    /// `ScriptPermissions::*` constants) by the client's responsibility for it.
    ///
    /// Returns `None` for a value that is not exactly one recognised flag bit
    /// (zero, an unknown bit, or several bits OR-ed together) — call it per set
    /// bit of a granted bitfield, not on the whole field.
    #[must_use]
    pub const fn for_flag(flag: i32) -> Option<Self> {
        match flag {
            ScriptPermissions::TAKE_CONTROLS
            | ScriptPermissions::TRACK_CAMERA
            | ScriptPermissions::CONTROL_CAMERA => Some(Self::Cooperation),
            ScriptPermissions::DEBIT
            | ScriptPermissions::TRIGGER_ANIMATION
            | ScriptPermissions::ATTACH
            | ScriptPermissions::CHANGE_LINKS
            | ScriptPermissions::TELEPORT
            | ScriptPermissions::EXPERIENCE
            | ScriptPermissions::SILENT_ESTATE_MANAGEMENT
            | ScriptPermissions::OVERRIDE_ANIMATIONS
            | ScriptPermissions::RETURN_OBJECTS => Some(Self::RecordOnly),
            _ => None,
        }
    }
}

/// A scripted-object permission request (`llRequestPermissions`), parsed from a
/// `ScriptQuestion`. Grant (a subset) with
/// [`Session::answer_script_permissions`](crate::Session::answer_script_permissions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPermissionRequest {
    /// The task (object) id holding the script.
    pub task_id: ObjectKey,
    /// The script item id within the object.
    pub item_id: InventoryKey,
    /// The object's name.
    pub object_name: String,
    /// The object owner's name.
    pub object_owner: String,
    /// The experience requesting, or `None` if the request is not made under an
    /// experience.
    pub experience_id: Option<ExperienceKey>,
    /// The permissions requested.
    pub permissions: ScriptPermissions,
}

/// A public, read-only view of one recorded script-permission grant, yielded by
/// [`Session::script_grants`](crate::Session::script_grants).
///
/// The grant registry's internal types stay private; this flattens what a driver
/// needs. The simulator stays authoritative — this only mirrors what the agent
/// granted, never a security boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptGrantInfo {
    /// The task (object) id holding the script.
    pub task_id: ObjectKey,
    /// The script item id within the object.
    pub item_id: InventoryKey,
    /// The granted permission subset. Empty when `denied` is set (an explicit
    /// deny grants nothing).
    pub granted: ScriptPermissions,
    /// `true` when the agent explicitly *denied* this script (answered with no
    /// permissions); `granted` is then empty. Distinct from a never-asked
    /// holder, which yields no [`ScriptGrantInfo`] at all.
    pub denied: bool,
    /// Whether the holder is one of this agent's attachments (the grant crosses
    /// regions with the avatar) rather than an in-world object.
    pub is_attachment: bool,
    /// The experience the grant was made under, or `None` outside an experience.
    pub experience_id: Option<ExperienceKey>,
}

/// The tri-state status of a script's permission request in the session's
/// permission mirror, returned by
/// [`Session::script_permission_status`](crate::Session::script_permission_status).
///
/// Distinguishes a script the agent has never been asked about from one it
/// explicitly denied — a distinction the driver's prompt UI needs (it may want
/// to surface "you previously refused this script"). The simulator stays
/// authoritative; this mirrors the agent's recorded answer, never a security
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPermissionStatus {
    /// No answer from this script's permission request has been recorded (the
    /// holder is absent from the mirror).
    NeverAsked,
    /// The agent explicitly denied this script (answered with no permissions).
    Denied,
    /// The agent granted this (non-empty) permission subset.
    Granted(ScriptPermissions),
}

/// A scripted-object request to open a URL (`llLoadURL`), parsed from a
/// `LoadURL`. There is no reply; the client decides whether to open the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadUrlRequest {
    /// The object's name.
    pub object_name: String,
    /// The object id.
    pub object_id: ObjectKey,
    /// The object's owner — an agent or a group.
    pub owner: OwnerKey,
    /// The accompanying message text.
    pub message: String,
    /// The URL the object asks to open.
    pub url: url::Url,
}

/// A scripted-object request to teleport the agent (`llMapDestination` /
/// `ScriptTeleportRequest`). There is no direct reply; the client may initiate a
/// teleport to the named region/position.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptTeleportRequest {
    /// The requesting object's name.
    pub object_name: String,
    /// The destination region (simulator) name, or `None` when the request
    /// carried an empty (unknown) name.
    pub region_name: Option<RegionName>,
    /// The destination position within the region, in metres.
    pub position: RegionCoordinates,
    /// The look-at direction on arrival.
    pub look_at: Direction,
    /// The request's option flags (`Options.Flags`). Reserved by the protocol;
    /// usually zero. The wire message carries a variable list of option blocks —
    /// this is the first block's flags (the only one a simulator sends).
    pub flags: u32,
}

/// A structured, localizable alert (`AlertInfo`): a message *key* the client
/// looks up in its `alerts.xml` (or equivalent) to produce a localized string,
/// together with the substitution parameters for that template. Carried by
/// messages such as `TeleportFailed` and `AlertMessage` alongside a plain
/// fallback string. Mirrors the viewer's `AlertInfo` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertInfo {
    /// The localizable message key (`Message`), e.g. `RegionEntryAccessBlocked`.
    /// Empty if the simulator sent no key.
    pub message: String,
    /// The substitution parameters for the keyed template (`ExtraParams`), as the
    /// raw string the simulator sent (a `key=value`/`|`-separated blob the viewer
    /// parses per-alert). Empty when the alert takes no parameters.
    pub extra_params: String,
}

/// Whether a scripted object is *taking* the named movement controls or
/// *releasing* them — the `TakeControls` wire flag on a `ScriptControlChange.Data`
/// block (`llTakeControls` vs `llReleaseControls`), modelled as a named intent
/// rather than a bare `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptControlAction {
    /// The script is *taking* the named controls (the `TakeControls` flag is
    /// set): route the control inputs to the script.
    Take,
    /// The script is *releasing* the named controls (the `TakeControls` flag is
    /// clear): stop routing them to the script.
    Release,
}

impl ScriptControlAction {
    /// Whether this action sets the `TakeControls` wire flag: `true` for
    /// [`Take`](Self::Take), `false` for [`Release`](Self::Release).
    #[must_use]
    pub const fn takes_controls(self) -> bool {
        matches!(self, Self::Take)
    }

    /// The action for a `TakeControls` flag bit: [`Take`](Self::Take) when set,
    /// [`Release`](Self::Release) when clear.
    #[must_use]
    pub const fn from_take_controls(take_controls: bool) -> Self {
        if take_controls {
            Self::Take
        } else {
            Self::Release
        }
    }
}

/// One control-grant change requested by a scripted object (`llTakeControls` /
/// `llReleaseControls`), parsed from one `ScriptControlChange.Data` block. The
/// sim sends this after the agent granted a script
/// [`ScriptPermissions::TAKE_CONTROLS`]; the client should route the named
/// control inputs to the script (and, when [`action`](Self::action) is
/// [`ScriptControlAction::Release`], stop doing so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptControl {
    /// Whether the script is *taking* the named controls or releasing them.
    pub action: ScriptControlAction,
    /// The movement-control bits the script is taking or releasing.
    pub controls: ControlFlags,
    /// Whether the named control inputs should still drive the agent in addition
    /// to being passed to the script (`PassToAgent`). When `false`, the script
    /// consumes them and the avatar does not move from them.
    pub pass_to_agent: bool,
}

/// A public, read-only snapshot of which movement controls scripts currently
/// hold, returned by [`Session::script_controls`](crate::Session::script_controls).
///
/// Mirrors the viewer's two taken-control sets, split by `PassToAgent`: controls
/// the script *consumes* (the avatar does not move from the input) versus
/// controls *also* passed to the agent. The session tracks this from the inbound
/// `ScriptControlChange` and clears it on
/// [`Session::release_script_controls`](crate::Session::release_script_controls).
/// The per-control take counts stay private; this exposes only the union of
/// currently-held bits. The simulator stays authoritative — this is an
/// API-convenience mirror, never a security boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptControlsInfo {
    /// Controls scripts hold and *consume* (`PassToAgent` was clear; the avatar
    /// does not move from these inputs). The union of every consumed control bit
    /// a script currently holds.
    pub taken: ControlFlags,
    /// Controls scripts hold that are *also* passed to the agent (`PassToAgent`
    /// was set; the input both drives the avatar and reaches the script). The
    /// union of every passed-on control bit a script currently holds.
    pub passed_to_agent: ControlFlags,
}

/// A complete, read-only snapshot of the session's script-permission mirror,
/// returned by
/// [`Session::script_permission_state`](crate::Session::script_permission_state)
/// and delivered to a driver as
/// [`Event::ScriptPermissionState`](crate::Event::ScriptPermissionState) in
/// reply to a [`Command::QueryScriptPermissions`](crate::Command::QueryScriptPermissions).
///
/// Bundles the two permission stores: every recorded grant/denial
/// ([`ScriptGrantInfo`], including explicit denials) and the currently-held
/// movement controls ([`ScriptControlsInfo`]). The simulator stays
/// authoritative — this is an API-convenience mirror of what the agent answered,
/// never a security boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPermissionState {
    /// Every recorded grant or denial, in deterministic order (a never-asked
    /// script is absent).
    pub grants: Vec<ScriptGrantInfo>,
    /// Which movement controls scripts currently hold, split by `PassToAgent`.
    pub controls: ScriptControlsInfo,
}

/// One follow-camera parameter a scripted object sets via `llSetCameraParams`,
/// the `Type` field of a `SetFollowCamProperties.CameraProperty` block. The
/// numeric values match the viewer's `EFollowCamAttributes`
/// (`llfollowcamparams.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FollowCamProperty {
    /// `FOLLOWCAM_PITCH` — camera pitch angle (degrees).
    Pitch,
    /// `FOLLOWCAM_FOCUS_OFFSET` — focus offset (sent as the X/Y/Z trio below).
    FocusOffset,
    /// `FOLLOWCAM_FOCUS_OFFSET_X` — focus offset X component.
    FocusOffsetX,
    /// `FOLLOWCAM_FOCUS_OFFSET_Y` — focus offset Y component.
    FocusOffsetY,
    /// `FOLLOWCAM_FOCUS_OFFSET_Z` — focus offset Z component.
    FocusOffsetZ,
    /// `FOLLOWCAM_POSITION_LAG` — camera position lag (seconds).
    PositionLag,
    /// `FOLLOWCAM_FOCUS_LAG` — camera focus lag (seconds).
    FocusLag,
    /// `FOLLOWCAM_DISTANCE` — camera distance from the focus (metres).
    Distance,
    /// `FOLLOWCAM_BEHINDNESS_ANGLE` — behindness angle (degrees).
    BehindnessAngle,
    /// `FOLLOWCAM_BEHINDNESS_LAG` — behindness lag (seconds).
    BehindnessLag,
    /// `FOLLOWCAM_POSITION_THRESHOLD` — position movement threshold (metres).
    PositionThreshold,
    /// `FOLLOWCAM_FOCUS_THRESHOLD` — focus movement threshold (metres).
    FocusThreshold,
    /// `FOLLOWCAM_ACTIVE` — whether the follow-camera is active (non-zero = on).
    Active,
    /// `FOLLOWCAM_POSITION` — camera position (sent as the X/Y/Z trio below).
    Position,
    /// `FOLLOWCAM_POSITION_X` — camera position X component.
    PositionX,
    /// `FOLLOWCAM_POSITION_Y` — camera position Y component.
    PositionY,
    /// `FOLLOWCAM_POSITION_Z` — camera position Z component.
    PositionZ,
    /// `FOLLOWCAM_FOCUS` — camera focus point (sent as the X/Y/Z trio below).
    Focus,
    /// `FOLLOWCAM_FOCUS_X` — camera focus X component.
    FocusX,
    /// `FOLLOWCAM_FOCUS_Y` — camera focus Y component.
    FocusY,
    /// `FOLLOWCAM_FOCUS_Z` — camera focus Z component.
    FocusZ,
    /// `FOLLOWCAM_POSITION_LOCKED` — whether the position is locked (non-zero).
    PositionLocked,
    /// `FOLLOWCAM_FOCUS_LOCKED` — whether the focus is locked (non-zero).
    FocusLocked,
    /// An unrecognised property type, preserved verbatim.
    Unknown(i32),
}

impl FollowCamProperty {
    /// Classifies a `CameraProperty.Type` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Pitch,
            1 => Self::FocusOffset,
            2 => Self::FocusOffsetX,
            3 => Self::FocusOffsetY,
            4 => Self::FocusOffsetZ,
            5 => Self::PositionLag,
            6 => Self::FocusLag,
            7 => Self::Distance,
            8 => Self::BehindnessAngle,
            9 => Self::BehindnessLag,
            10 => Self::PositionThreshold,
            11 => Self::FocusThreshold,
            12 => Self::Active,
            13 => Self::Position,
            14 => Self::PositionX,
            15 => Self::PositionY,
            16 => Self::PositionZ,
            17 => Self::Focus,
            18 => Self::FocusX,
            19 => Self::FocusY,
            20 => Self::FocusZ,
            21 => Self::PositionLocked,
            22 => Self::FocusLocked,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this property type.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::Pitch => 0,
            Self::FocusOffset => 1,
            Self::FocusOffsetX => 2,
            Self::FocusOffsetY => 3,
            Self::FocusOffsetZ => 4,
            Self::PositionLag => 5,
            Self::FocusLag => 6,
            Self::Distance => 7,
            Self::BehindnessAngle => 8,
            Self::BehindnessLag => 9,
            Self::PositionThreshold => 10,
            Self::FocusThreshold => 11,
            Self::Active => 12,
            Self::Position => 13,
            Self::PositionX => 14,
            Self::PositionY => 15,
            Self::PositionZ => 16,
            Self::Focus => 17,
            Self::FocusX => 18,
            Self::FocusY => 19,
            Self::FocusZ => 20,
            Self::PositionLocked => 21,
            Self::FocusLocked => 22,
            Self::Unknown(other) => other,
        }
    }
}

/// One follow-camera parameter and its value, from a
/// `SetFollowCamProperties.CameraProperty` block (`llSetCameraParams`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowCamPropertyValue {
    /// Which camera parameter this sets.
    pub property: FollowCamProperty,
    /// The parameter's value (interpretation depends on
    /// [`property`](Self::property) — angle, distance, lag, boolean flag, …).
    pub value: f32,
}

/// The kind of thing a mute-list entry blocks, from the `MuteType` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MuteType {
    /// A mute by display name only (no specific id).
    ByName,
    /// A muted agent (avatar).
    Agent,
    /// A muted object.
    Object,
    /// A muted group.
    Group,
    /// A muted external (e.g. hypergrid) entity.
    External,
    /// An unrecognised mute-type value, preserved verbatim.
    Unknown(i32),
}

impl MuteType {
    /// Classifies a `MuteType` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::ByName,
            1 => Self::Agent,
            2 => Self::Object,
            3 => Self::Group,
            4 => Self::External,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this mute type.
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::ByName => 0,
            Self::Agent => 1,
            Self::Object => 2,
            Self::Group => 3,
            Self::External => 4,
            Self::Unknown(other) => other,
        }
    }
}

/// The per-entry mute flags bitfield. **Each set bit is an *exception*** — it
/// means "do *not* mute this aspect" — so `MuteFlags(0)` mutes everything (the
/// usual case). The flag values match the viewer's `LLMute::flag*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MuteFlags(pub u32);

impl MuteFlags {
    /// Do not mute the target's text chat.
    pub const ALLOW_TEXT_CHAT: u32 = 0x1;
    /// Do not mute the target's voice chat.
    pub const ALLOW_VOICE_CHAT: u32 = 0x2;
    /// Do not mute the target's particles.
    pub const ALLOW_PARTICLES: u32 = 0x4;
    /// Do not mute the target's object sounds.
    pub const ALLOW_OBJECT_SOUNDS: u32 = 0x8;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

/// One entry in the agent's mute (block) list, parsed from the downloaded mute
/// file ([`Event::MuteList`](crate::Event::MuteList)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuteEntry {
    /// The muted entity's id (nil for a [`MuteType::ByName`] mute).
    pub id: Uuid,
    /// The muted entity's name.
    pub name: String,
    /// What kind of entity is muted.
    pub mute_type: MuteType,
    /// The per-entry exception flags.
    pub flags: MuteFlags,
}

// ---------------------------------------------------------------------------
// Script upload & compilation control (`UpdateScriptAgent` / `UpdateScriptTask`).
// ---------------------------------------------------------------------------

/// The compiler / runtime backend a script upload asks the simulator to compile
/// for — the `target` field of an `UpdateScriptAgent` / `UpdateScriptTask`
/// capability POST.
///
/// The **simulator** compiles; this only *requests* a backend (the viewer never
/// compiles locally). Second Life honours the token; OpenSim ignores it and picks
/// the language from a source-header comment, so an unknown backend does no harm
/// there.
///
/// `#[non_exhaustive]`: Linden Lab's own viewer moved this from a fixed
/// `{ LSL2, MONO }` enum to a free-form combo whose values now include `luau`
/// (Lua/SLua), with more runtimes (e.g. a new LSL VM) in progress. New backends
/// arrive rarely (every few years), so each is added as a variant when it ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScriptTarget {
    /// Legacy LSL bytecode (`"lsl2"`).
    Lsl2,
    /// Mono / CIL (`"mono"`) — the Second Life default for LSL.
    Mono,
    /// Lua / SLua, compiled with Luau (`"luau"`).
    Luau,
}

impl ScriptTarget {
    /// The wire token for this backend (the `target` field value).
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::Lsl2 => "lsl2",
            Self::Mono => "mono",
            Self::Luau => "luau",
        }
    }

    /// Classifies a `target` wire token, or `None` for one this build does not
    /// know (a backend Linden Lab has added but this crate has not yet modelled).
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            "lsl2" => Some(Self::Lsl2),
            "mono" => Some(Self::Mono),
            "luau" => Some(Self::Luau),
            _ => None,
        }
    }
}

/// The scripting language a script inventory item is written in, carried in the
/// low byte of the item's `flags` (`II_FLAGS_SUBTYPE_MASK`), as Linden Lab's
/// viewer records it (`ScriptSubtype_t`). Distinct from [`ScriptTarget`], which
/// is a per-upload *compile* request; this is a persisted property of the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScriptLanguage {
    /// LSL (`SST_LSL = 0`).
    Lsl,
    /// Lua / SLua (`SST_LUA = 1`).
    Luau,
}

impl ScriptLanguage {
    /// The item-`flags` low-byte mask carrying the script subtype
    /// (`II_FLAGS_SUBTYPE_MASK`).
    pub const SUBTYPE_MASK: u32 = 0x0000_00ff;

    /// The `ScriptSubtype_t` byte for this language (`SST_LSL`/`SST_LUA`).
    #[must_use]
    pub const fn subtype(self) -> u8 {
        match self {
            Self::Lsl => 0,
            Self::Luau => 1,
        }
    }

    /// Classifies a `ScriptSubtype_t` byte, or `None` for an unknown subtype.
    #[must_use]
    pub const fn from_subtype(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Lsl),
            1 => Some(Self::Luau),
            _ => None,
        }
    }

    /// The language recorded in an inventory item's `flags`, reading the subtype
    /// low byte ([`SUBTYPE_MASK`](Self::SUBTYPE_MASK)); `None` for an unknown
    /// subtype.
    #[must_use]
    pub fn from_item_flags(flags: u32) -> Option<Self> {
        let byte = u8::try_from(flags & Self::SUBTYPE_MASK).ok()?;
        Self::from_subtype(byte)
    }
}

/// Where a script's source is being uploaded to — selecting the capability and
/// the request body for [`Command::UploadScript`](crate::Command::UploadScript).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptUploadLocation {
    /// A script item in the agent's own inventory (`UpdateScriptAgent`).
    AgentInventory {
        /// The script inventory item whose source is being replaced.
        item_id: InventoryKey,
    },
    /// A script item inside an in-world object's task inventory
    /// (`UpdateScriptTask`).
    TaskInventory {
        /// The object (task) holding the script.
        task_id: ObjectKey,
        /// The script item within that object's inventory.
        item_id: InventoryKey,
        /// Whether the script should be running after the update
        /// (`is_script_running`).
        running: bool,
        /// The experience the script runs under, or `None` outside an experience.
        experience: Option<ExperienceKey>,
    },
}

/// One compiler diagnostic from a script upload, parsed (best-effort) out of one
/// entry of the capability response's `errors` array.
///
/// The [`raw`](Self::raw) string is always preserved verbatim; [`line`](Self::line)
/// / [`column`](Self::column) / [`message`](Self::message) are a best-effort split
/// of the grid's format (Second Life Mono and OpenSim XEngine format these
/// differently), falling back to `line`/`column` `None` and `message == raw` when
/// no position prefix is recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCompileError {
    /// The diagnostic string exactly as the simulator sent it.
    pub raw: String,
    /// The 1-based source line, if a position prefix was recognised.
    pub line: Option<u32>,
    /// The source column, if a position prefix was recognised.
    pub column: Option<u32>,
    /// The human-readable message (the diagnostic with any position prefix
    /// stripped, or the whole string when none was found).
    pub message: String,
}

impl ScriptCompileError {
    /// Parses one compiler-error string into a structured diagnostic, keeping the
    /// original in [`raw`](Self::raw). Recognises a leading `(line, col)` prefix
    /// (OpenSim / Mono) or a `line:col:` prefix, else leaves the position empty
    /// and the whole string as the message.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if let Some((line, column, message)) = parse_paren_position(trimmed) {
            return Self {
                raw: raw.to_owned(),
                line: Some(line),
                column: Some(column),
                message,
            };
        }
        if let Some((line, column, message)) = parse_colon_position(trimmed) {
            return Self {
                raw: raw.to_owned(),
                line: Some(line),
                column: Some(column),
                message,
            };
        }
        Self {
            raw: raw.to_owned(),
            line: None,
            column: None,
            message: trimmed.to_owned(),
        }
    }

    /// Renders this grid-side compiler error against the script `source` through
    /// `sl-lsl`'s diagnostic machinery — the *same* renderer a locally-found
    /// diagnostic uses — so a caret underlines the reported position over the
    /// real source line instead of the bare `(line, col)` the simulator sent.
    ///
    /// When no position prefix was recognised ([`line`](Self::line) is `None`),
    /// the caret points at the start of the source; the message still renders.
    /// The result is a multi-line block suitable for a log or an editor's error
    /// panel.
    #[must_use]
    pub fn render(&self, source: &str) -> String {
        sl_lsl::render_grid_error(
            source,
            self.line.unwrap_or(1),
            self.column,
            sl_lsl::Severity::Error,
            &self.message,
        )
    }
}

/// Parses a leading `(line, col)` position (e.g. `"(4, 20): message"`, as OpenSim
/// XEngine and SL Mono emit), returning `(line, col, message)` with the prefix
/// stripped. The message keeps whatever follows the closing paren (minus a
/// leading `:`/whitespace).
fn parse_paren_position(s: &str) -> Option<(u32, u32, String)> {
    let rest = s.strip_prefix('(')?;
    let (inside, after) = rest.split_once(')')?;
    let mut nums = inside.split(',');
    let line = nums.next()?.trim().parse::<u32>().ok()?;
    let column = nums.next()?.trim().parse::<u32>().ok()?;
    let message = after
        .trim_start_matches(|c: char| c == ':' || c.is_whitespace())
        .to_owned();
    Some((line, column, message))
}

/// Parses a leading `line:col:` position (e.g. `"12:3: message"`), returning
/// `(line, col, message)` with the prefix stripped.
fn parse_colon_position(s: &str) -> Option<(u32, u32, String)> {
    let mut parts = s.splitn(3, ':');
    let line = parts.next()?.trim().parse::<u32>().ok()?;
    let column = parts.next()?.trim().parse::<u32>().ok()?;
    let message = parts.next()?.trim_start().to_owned();
    Some((line, column, message))
}

/// A compilable default LSL script body, matching what a viewer's "New Script"
/// leaves in a fresh item (the classic Second Life starter). Handy for seeding a
/// body through [`Command::UploadScript`](crate::Command::UploadScript) so an
/// empty (non-compiling) source is never uploaded. Creating an item with
/// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
/// already gets the simulator's own default body; this is for the explicit-seed
/// path.
pub const DEFAULT_LSL_SCRIPT: &str = "default\n\
    {\n\
    \x20   state_entry()\n\
    \x20   {\n\
    \x20       llSay(0, \"Hello, Avatar!\");\n\
    \x20   }\n\
    \n\
    \x20   touch_start(integer total_number)\n\
    \x20   {\n\
    \x20       llSay(0, \"Touched.\");\n\
    \x20   }\n\
    }\n";

/// A compilable default Lua/SLua script body — a comment-only program, which is
/// valid Luau (unlike an empty LSL script, an empty Luau program compiles). Used
/// like [`DEFAULT_LSL_SCRIPT`] to seed a non-empty body for a `luau`
/// [`ScriptTarget`] upload.
pub const DEFAULT_LUAU_SCRIPT: &str = "-- SLua script\n";

#[cfg(test)]
mod tests {
    use super::{
        ChatChannel, PermissionRole, ScriptCompileError, ScriptControlAction, ScriptLanguage,
        ScriptPermissions, ScriptTarget,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn permission_role_classifies_representative_flags() {
        // The three cooperation flags: the runtime routes inputs / camera.
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::TAKE_CONTROLS),
            Some(PermissionRole::Cooperation)
        );
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::TRACK_CAMERA),
            Some(PermissionRole::Cooperation)
        );
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::CONTROL_CAMERA),
            Some(PermissionRole::Cooperation)
        );
        // Representative record-only flags, including `TELEPORT` (enforced
        // server-side, not a client action — see `PermissionRole`).
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::DEBIT),
            Some(PermissionRole::RecordOnly)
        );
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::TELEPORT),
            Some(PermissionRole::RecordOnly)
        );
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::OVERRIDE_ANIMATIONS),
            Some(PermissionRole::RecordOnly)
        );
        // Not exactly one recognised flag bit: zero, two bits at once, an
        // unknown bit (`1 << 0` is reserved/unused in the LSL constants).
        assert_eq!(PermissionRole::for_flag(0), None);
        assert_eq!(
            PermissionRole::for_flag(ScriptPermissions::DEBIT | ScriptPermissions::TAKE_CONTROLS),
            None
        );
        assert_eq!(PermissionRole::for_flag(1 << 0), None);
    }

    #[test]
    fn script_control_action_maps_to_take_controls_flag() {
        assert!(ScriptControlAction::Take.takes_controls());
        assert!(!ScriptControlAction::Release.takes_controls());
        assert_eq!(
            ScriptControlAction::from_take_controls(true),
            ScriptControlAction::Take
        );
        assert_eq!(
            ScriptControlAction::from_take_controls(false),
            ScriptControlAction::Release
        );
        // The action round-trips bit-identically to the historical `bool`.
        for action in [ScriptControlAction::Take, ScriptControlAction::Release] {
            assert_eq!(
                ScriptControlAction::from_take_controls(action.takes_controls()),
                action
            );
        }
    }

    #[test]
    fn chat_channel_round_trips_raw_i32() {
        // The typed channel wraps the raw wire `i32` bit-identically, including
        // the negative hidden channels scripts use for dialog replies.
        for raw in [0_i32, 5, -1234, i32::MIN, i32::MAX] {
            assert_eq!(ChatChannel(raw).0, raw);
        }
    }

    #[test]
    fn script_target_wire_round_trips() {
        for target in [ScriptTarget::Lsl2, ScriptTarget::Mono, ScriptTarget::Luau] {
            assert_eq!(ScriptTarget::from_wire(target.to_wire()), Some(target));
        }
        assert_eq!(ScriptTarget::Luau.to_wire(), "luau");
        // An unknown backend (e.g. one LL adds later) is not silently coerced.
        assert_eq!(ScriptTarget::from_wire("lso2"), None);
        assert_eq!(ScriptTarget::from_wire(""), None);
    }

    #[test]
    fn script_language_subtype_and_item_flags() {
        assert_eq!(ScriptLanguage::Lsl.subtype(), 0);
        assert_eq!(ScriptLanguage::Luau.subtype(), 1);
        assert_eq!(ScriptLanguage::from_subtype(0), Some(ScriptLanguage::Lsl));
        assert_eq!(ScriptLanguage::from_subtype(1), Some(ScriptLanguage::Luau));
        assert_eq!(ScriptLanguage::from_subtype(2), None);
        // The subtype lives in the low byte of the item flags; higher bits are
        // other flag bits and must be ignored.
        assert_eq!(
            ScriptLanguage::from_item_flags(0xff00 | 0x01),
            Some(ScriptLanguage::Luau)
        );
        assert_eq!(
            ScriptLanguage::from_item_flags(0xdead_0000),
            Some(ScriptLanguage::Lsl)
        );
    }

    #[test]
    fn script_compile_error_parses_opensim_and_mono_formats() {
        // OpenSim / Mono parenthesised position.
        let paren = ScriptCompileError::parse("(4, 20): ERROR: Syntax error");
        assert_eq!(paren.line, Some(4));
        assert_eq!(paren.column, Some(20));
        assert_eq!(paren.message, "ERROR: Syntax error");
        assert_eq!(paren.raw, "(4, 20): ERROR: Syntax error");

        // `line:col:` position.
        let colon = ScriptCompileError::parse("12:3: unexpected token");
        assert_eq!(colon.line, Some(12));
        assert_eq!(colon.column, Some(3));
        assert_eq!(colon.message, "unexpected token");

        // No recognised position — whole (trimmed) string is the message, raw
        // preserved verbatim.
        let plain = ScriptCompileError::parse("  could not compile  ");
        assert_eq!(plain.line, None);
        assert_eq!(plain.column, None);
        assert_eq!(plain.message, "could not compile");
        assert_eq!(plain.raw, "  could not compile  ");
    }

    #[test]
    fn script_compile_error_renders_against_source() {
        // A grid error at line 3, column 5 renders through sl-lsl's renderer:
        // the header, the `-->` locator, the real source line and a caret.
        let source = "default\n{\n    bogus();\n}\n";
        let error = ScriptCompileError::parse("(3, 5): ERROR: Syntax error");
        let rendered = error.render(source);
        assert!(rendered.contains("error: ERROR: Syntax error"));
        assert!(rendered.contains("--> 3:5"));
        assert!(rendered.contains("    bogus();"));
        assert!(rendered.contains('^'));

        // With no recognised position the message still renders (caret at the
        // source start).
        let positionless = ScriptCompileError::parse("could not compile");
        let rendered = positionless.render(source);
        assert!(rendered.contains("error: could not compile"));
        assert!(rendered.contains("--> 1:1"));
    }
}
