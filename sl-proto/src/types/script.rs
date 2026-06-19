//! Scripts and notifications: dialogs, permissions, alerts, mutes.

use uuid::Uuid;

/// A scripted-object dialog (`llDialog`/`llTextBox`), parsed from a
/// `ScriptDialog`. Reply with
/// [`Session::reply_script_dialog`](crate::Session::reply_script_dialog), passing
/// the chosen button's index/label on [`chat_channel`](ScriptDialog::chat_channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDialog {
    /// The object id that raised the dialog (the reply target).
    pub object_id: Uuid,
    /// The object's name.
    pub object_name: String,
    /// The object owner's first name.
    pub owner_first_name: String,
    /// The object owner's last name.
    pub owner_last_name: String,
    /// The object owner's agent id (nil if the sim did not include it).
    pub owner_id: Uuid,
    /// The dialog message text.
    pub message: String,
    /// The hidden chat channel the button reply is sent on.
    pub chat_channel: i32,
    /// The dialog's icon (texture id).
    pub image_id: Uuid,
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

/// The permissions an in-world script may request via `llRequestPermissions`, a
/// bitfield shared by `ScriptQuestion` (request) and `ScriptAnswerYes` (grant).
/// The flag values match the LSL `PERMISSION_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScriptPermissions(pub i32);

impl ScriptPermissions {
    /// Debit the agent's account (`PERMISSION_DEBIT`).
    pub const DEBIT: i32 = 1 << 1;
    /// Take control inputs (`PERMISSION_TAKE_CONTROLS`).
    pub const TAKE_CONTROLS: i32 = 1 << 2;
    /// Trigger animations on the agent (`PERMISSION_TRIGGER_ANIMATION`).
    pub const TRIGGER_ANIMATION: i32 = 1 << 4;
    /// Attach to the agent (`PERMISSION_ATTACH`).
    pub const ATTACH: i32 = 1 << 5;
    /// Change link-set membership (`PERMISSION_CHANGE_LINKS`).
    pub const CHANGE_LINKS: i32 = 1 << 7;
    /// Track the agent's camera (`PERMISSION_TRACK_CAMERA`).
    pub const TRACK_CAMERA: i32 = 1 << 10;
    /// Control the agent's camera (`PERMISSION_CONTROL_CAMERA`).
    pub const CONTROL_CAMERA: i32 = 1 << 11;
    /// Teleport the agent (`PERMISSION_TELEPORT`).
    pub const TELEPORT: i32 = 1 << 12;
    /// Participate in an experience (`PERMISSION_EXPERIENCE`).
    pub const EXPERIENCE: i32 = 1 << 13;
    /// Silently manage estate access (`PERMISSION_SILENT_ESTATE_MANAGEMENT`).
    pub const SILENT_ESTATE_MANAGEMENT: i32 = 1 << 14;
    /// Override the agent's animations (`PERMISSION_OVERRIDE_ANIMATIONS`).
    pub const OVERRIDE_ANIMATIONS: i32 = 1 << 15;
    /// Return objects (`PERMISSION_RETURN_OBJECTS`).
    pub const RETURN_OBJECTS: i32 = 1 << 16;

    /// Whether all of the bits in `mask` are granted/requested.
    #[must_use]
    pub const fn contains(self, mask: i32) -> bool {
        self.0 & mask == mask
    }
}

/// A scripted-object permission request (`llRequestPermissions`), parsed from a
/// `ScriptQuestion`. Grant (a subset) with
/// [`Session::answer_script_permissions`](crate::Session::answer_script_permissions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPermissionRequest {
    /// The task (object) id holding the script.
    pub task_id: Uuid,
    /// The script item id within the object.
    pub item_id: Uuid,
    /// The object's name.
    pub object_name: String,
    /// The object owner's name.
    pub object_owner: String,
    /// The experience id requesting, or nil if not an experience.
    pub experience_id: Uuid,
    /// The permissions requested.
    pub permissions: ScriptPermissions,
}

/// A scripted-object request to open a URL (`llLoadURL`), parsed from a
/// `LoadURL`. There is no reply; the client decides whether to open the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadUrlRequest {
    /// The object's name.
    pub object_name: String,
    /// The object id.
    pub object_id: Uuid,
    /// The object owner's agent (or group) id.
    pub owner_id: Uuid,
    /// Whether [`owner_id`](Self::owner_id) is a group rather than an agent.
    pub owner_is_group: bool,
    /// The accompanying message text.
    pub message: String,
    /// The URL the object asks to open.
    pub url: String,
}

/// A scripted-object request to teleport the agent (`llMapDestination` /
/// `ScriptTeleportRequest`). There is no direct reply; the client may initiate a
/// teleport to the named region/position.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptTeleportRequest {
    /// The requesting object's name.
    pub object_name: String,
    /// The destination region (simulator) name.
    pub region_name: String,
    /// The destination position within the region, in metres.
    pub position: (f32, f32, f32),
    /// The look-at direction on arrival.
    pub look_at: (f32, f32, f32),
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

/// The kind of thing a mute-list entry blocks, from the `MuteType` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
