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

#[cfg(test)]
mod tests {
    use super::{ChatChannel, PermissionRole, ScriptControlAction, ScriptPermissions};
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
}
